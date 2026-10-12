//! Remplissage d'un gabarit : recherche en profondeur sur les emplacements,
//! heuristique MRV (emplacement le plus contraint d'abord), vérification en
//! avant sur les emplacements croisés, pas de mot répété, budget de nœuds pour
//! permettre les redémarrages aléatoires.

use crate::dict::{Bucket, Dictionary, Letter};
use crate::grid::{Cell, Grid, Slot, Solution};
use crate::rng::Rng;

/// Résultat d'une tentative de remplissage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Solved,
    /// Espace de recherche épuisé : aucune solution n'existe.
    Impossible,
    /// Budget de nœuds atteint avant de conclure.
    Budget,
}

const EMPTY: u8 = u8::MAX;

enum Trail {
    Letter(usize),
    Used(usize, u32),
    Count(usize, u32),
}

pub(crate) struct Solver<'d> {
    dict: &'d Dictionary,
    grid: Grid,
    slots: Vec<Slot>,
    /// Pour chaque case : (emplacement, position) horizontal puis vertical.
    cell_slots: Vec<[Option<(usize, usize)>; 2]>,
    letters: Vec<u8>,
    filled: Vec<usize>,
    counts: Vec<u32>,
    /// Mots déjà placés, un bitset par longueur.
    used: Vec<Vec<u64>>,
    trail: Vec<Trail>,
    rng: Rng,
    nodes: u64,
    budget: u64,
    pattern_buf: Vec<Option<Letter>>,
}

impl<'d> Solver<'d> {
    /// Prépare le solveur. Renvoie `None` si le gabarit est déjà insoluble
    /// (longueur absente du dictionnaire, mot imposé inconnu ou répété...).
    pub(crate) fn new(dict: &'d Dictionary, grid: &Grid, rng: Rng, budget: u64) -> Option<Self> {
        let slots = grid.slots();
        let mut cell_slots = vec![[None, None]; grid.len()];
        for (si, s) in slots.iter().enumerate() {
            let dir = usize::from(!s.across);
            for (p, &c) in s.cells.iter().enumerate() {
                cell_slots[c][dir] = Some((si, p));
            }
        }
        let used = dict.buckets.iter().map(|b| vec![0u64; b.nw]).collect();
        let mut letters = vec![EMPTY; grid.len()];
        for (i, l) in letters.iter_mut().enumerate() {
            if let Cell::Open(Some(x)) = grid.cell(i) {
                *l = x;
            }
        }
        let filled = slots
            .iter()
            .map(|s| s.cells.iter().filter(|&&c| letters[c] != EMPTY).count())
            .collect();
        let mut solver = Solver {
            dict,
            grid: grid.clone(),
            counts: vec![0; slots.len()],
            slots,
            cell_slots,
            letters,
            filled,
            used,
            trail: Vec::new(),
            rng,
            nodes: 0,
            budget,
            pattern_buf: Vec::new(),
        };
        for si in 0..solver.slots.len() {
            let len = solver.slots[si].len();
            solver.dict.bucket(len)?;
            if solver.filled[si] == len {
                // Mot entièrement imposé : il doit exister et ne pas être répété.
                solver.claim_full(si)?;
            } else {
                solver.counts[si] = solver.count(si);
                if solver.counts[si] == 0 {
                    return None;
                }
            }
        }
        solver.trail.clear();
        Some(solver)
    }

    fn bucket(&self, si: usize) -> &'d Bucket {
        self.dict
            .bucket(self.slots[si].len())
            .expect("longueur vérifiée à la construction")
    }

    fn load_pattern(&mut self, si: usize) {
        self.pattern_buf.clear();
        for &c in &self.slots[si].cells {
            let l = self.letters[c];
            self.pattern_buf.push((l != EMPTY).then_some(l));
        }
    }

    fn count(&mut self, si: usize) -> u32 {
        self.load_pattern(si);
        let b = self.bucket(si);
        b.count_matching(&self.pattern_buf, &self.used[b.len])
    }

    /// Enregistre comme utilisé le mot d'un emplacement complet.
    /// `None` s'il n'est pas dans le dictionnaire ou déjà employé.
    fn claim_full(&mut self, si: usize) -> Option<()> {
        self.load_pattern(si);
        let b = self.bucket(si);
        let idx = *b.matching(&self.pattern_buf, &self.used[b.len]).first()?;
        self.used[b.len][idx as usize / 64] |= 1 << (idx % 64);
        self.trail.push(Trail::Used(b.len, idx));
        Some(())
    }

    /// Place le mot `idx` dans l'emplacement `si` et propage. Renvoie `false`
    /// si un emplacement croisé devient impossible (l'appelant défait la piste).
    fn assign(&mut self, si: usize, idx: u32) -> bool {
        let b = self.bucket(si);
        self.used[b.len][idx as usize / 64] |= 1 << (idx % 64);
        self.trail.push(Trail::Used(b.len, idx));
        let word = b.word(idx as usize);
        let dir = usize::from(!self.slots[si].across);
        let mut crossings = Vec::new();
        for (p, &c) in self.slots[si].cells.iter().enumerate() {
            if self.letters[c] != EMPTY {
                continue;
            }
            self.letters[c] = word[p];
            self.trail.push(Trail::Letter(c));
            for (s, _) in self.cell_slots[c].iter().flatten() {
                self.filled[*s] += 1;
            }
            if let Some((t, _)) = self.cell_slots[c][1 - dir] {
                crossings.push(t);
            }
        }
        for t in crossings {
            if self.filled[t] == self.slots[t].len() {
                if self.claim_full(t).is_none() {
                    return false;
                }
            } else {
                let n = self.count(t);
                self.trail.push(Trail::Count(t, self.counts[t]));
                self.counts[t] = n;
                if n == 0 {
                    return false;
                }
            }
        }
        true
    }

    fn undo_to(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop().expect("piste non vide") {
                Trail::Letter(c) => {
                    self.letters[c] = EMPTY;
                    for (s, _) in self.cell_slots[c].iter().flatten() {
                        self.filled[*s] -= 1;
                    }
                }
                Trail::Used(len, idx) => self.used[len][idx as usize / 64] &= !(1 << (idx % 64)),
                Trail::Count(s, old) => self.counts[s] = old,
            }
        }
    }

    /// Emplacement incomplet ayant le moins de candidats ; à égalité, le plus
    /// long, puis le premier rencontré à partir d'un point de départ aléatoire.
    fn pick_slot(&mut self) -> Option<usize> {
        let n = self.slots.len();
        let start = self.rng.below(n.max(1));
        let mut best: Option<(u32, std::cmp::Reverse<usize>, usize)> = None;
        for k in 0..n {
            let si = (start + k) % n;
            if self.filled[si] == self.slots[si].len() {
                continue;
            }
            let key = (self.counts[si], std::cmp::Reverse(self.slots[si].len()), si);
            if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                best = Some(key);
            }
        }
        best.map(|b| b.2)
    }

    fn search(&mut self) -> Outcome {
        let Some(si) = self.pick_slot() else {
            return Outcome::Solved;
        };
        self.load_pattern(si);
        let b = self.bucket(si);
        let mut cands = b.matching(&self.pattern_buf, &self.used[b.len]);
        // Tirage aléatoire sans remise, avec un tournoi à deux pour favoriser
        // les mots de meilleur score quand le dictionnaire en fournit.
        let n = cands.len();
        for i in 0..n {
            if self.nodes >= self.budget {
                return Outcome::Budget;
            }
            self.nodes += 1;
            let rem = n - i;
            let a = i + self.rng.below(rem);
            let c = i + self.rng.below(rem);
            let pick = if b.scores[cands[c] as usize] > b.scores[cands[a] as usize] {
                c
            } else {
                a
            };
            cands.swap(i, pick);
            let mark = self.trail.len();
            if self.assign(si, cands[i]) {
                match self.search() {
                    Outcome::Impossible => {}
                    other => return other,
                }
            }
            self.undo_to(mark);
        }
        Outcome::Impossible
    }

    /// Lance la recherche ; en cas de succès renvoie la grille remplie.
    pub(crate) fn run(mut self) -> (Outcome, Option<Solution>, u64) {
        let outcome = self.search();
        let sol = (outcome == Outcome::Solved).then(|| {
            let mut g = self.grid.clone();
            for (i, &l) in self.letters.iter().enumerate() {
                if l != EMPTY {
                    g.set_cell(i, Cell::Open(Some(l)));
                }
            }
            Solution::from_grid(g)
        });
        (outcome, sol, self.nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_small_square() {
        // Carré 3×3 sans noire : ABC/DEF/GHI horizontalement, ADG/BEH/CFI
        // verticalement, noyés parmi des leurres.
        let dict = Dictionary::from_words([
            "ABC", "DEF", "GHI", "ADG", "BEH", "CFI", "ABD", "XYZ", "GEF", "AAA", "CHI",
        ]);
        let grid = Grid::parse("...\n...\n...").unwrap();
        for seed in 0..20 {
            let s = Solver::new(&dict, &grid, Rng::new(seed), 100_000).unwrap();
            let (outcome, sol, _) = s.run();
            assert_eq!(outcome, Outcome::Solved);
            let rows: Vec<_> = sol.unwrap().across().map(|e| e.word.clone()).collect();
            assert!(
                rows == ["ABC", "DEF", "GHI"] || rows == ["ADG", "BEH", "CFI"],
                "{rows:?}"
            );
        }
    }

    #[test]
    fn proves_impossibility() {
        // 2×2 sans noire = 4 mots distincts nécessaires, un seul disponible.
        let dict = Dictionary::from_words(["AA"]);
        let grid = Grid::parse("..\n..").unwrap();
        let (outcome, sol, _) = Solver::new(&dict, &grid, Rng::new(0), 1000).unwrap().run();
        assert_eq!(outcome, Outcome::Impossible);
        assert!(sol.is_none());
    }

    #[test]
    fn fills_with_crossings() {
        // Grille 3×3 avec un coin noir et deux mots de 2 lettres.
        //   . . #
        //   . . .
        //   # . .
        // Solution attendue : ON / RUT / SE et OR / NUS / TE (plus des leurres).
        let dict = Dictionary::from_words([
            "ON", "RUT", "SE", "OR", "NUS", "TE", "LA", "ET", "ANE", "NU",
        ]);
        let grid = Grid::parse("..#\n...\n#..").unwrap();
        let s = Solver::new(&dict, &grid, Rng::new(1), 100_000).unwrap();
        let (outcome, sol, _) = s.run();
        assert_eq!(outcome, Outcome::Solved);
        let sol = sol.unwrap();
        let mut seen = std::collections::HashSet::new();
        for e in &sol.entries {
            assert!(dict.contains(&e.word), "{} absent du dictionnaire", e.word);
            assert!(seen.insert(e.word.clone()), "{} répété", e.word);
        }
    }

    #[test]
    fn respects_imposed_letters() {
        let dict = Dictionary::from_words(["AB", "AC", "BD", "CD", "CB", "DB"]);
        let grid = Grid::parse("A.\n..").unwrap();
        let s = Solver::new(&dict, &grid, Rng::new(9), 10_000).unwrap();
        let (outcome, sol, _) = s.run();
        assert_eq!(outcome, Outcome::Solved);
        let g = sol.unwrap().grid;
        assert_eq!(g.get(0, 0), Cell::Open(Some(0)));
    }

    #[test]
    fn unknown_imposed_word_is_rejected() {
        let dict = Dictionary::from_words(["AB"]);
        let grid = Grid::parse("ZZ").unwrap();
        assert!(Solver::new(&dict, &grid, Rng::new(0), 10).is_none());
    }
}
