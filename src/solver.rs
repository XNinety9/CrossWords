//! Remplissage d'un gabarit par propagation de contraintes et recherche en
//! profondeur.
//!
//! Chaque case porte l'ensemble des lettres encore possibles, chaque
//! emplacement le bitset de ses mots candidats. Toute réduction se propage
//! jusqu'au point fixe (cohérence d'arc entre emplacements croisés) : une
//! case dont l'ensemble se vide signale l'impasse plusieurs coups à l'avance.
//! On choisit l'emplacement le plus contraint (MRV) ; un mot réfuté est retiré
//! de l'emplacement pour le reste de la branche, ce qui relance la
//! propagation. Aucun mot n'est employé deux fois. Un budget de nœuds permet
//! les redémarrages aléatoires.

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

const ALL_LETTERS: u32 = (1 << 26) - 1;

/// Au-delà de ce nombre de candidats, on ne recalcule pas les lettres
/// possibles d'un emplacement : elles le sont presque toutes, et le calcul
/// coûterait plus qu'il ne rapporte.
const LETTER_CAP: u32 = 2048;

/// Amplitude du bruit aléatoire dans l'ordre des candidats.
const NOISE: f32 = 2.0;

enum Trail {
    Dom(usize, u32),
    Cand {
        slot: usize,
        at: usize,
        count: u32,
        saved_at: u32,
    },
    Assigned(usize),
}

pub(crate) struct Solver<'d> {
    grid: Grid,
    slots: Vec<Slot>,
    buckets: Vec<&'d Bucket>,
    /// Pour chaque case : (emplacement, position) horizontal puis vertical.
    cell_slots: Vec<[Option<(usize, usize)>; 2]>,
    /// Emplacements regroupés par longueur, pour l'unicité des mots.
    by_len: Vec<Vec<usize>>,
    /// Lettres possibles par case (bit `l` pour la lettre `l`).
    dom: Vec<u32>,
    cand: Vec<Vec<u64>>,
    count: Vec<u32>,
    assigned: Vec<Option<u32>>,
    /// Niveau auquel `cand[s]` a été sauvegardé pour la dernière fois.
    saved_at: Vec<u32>,
    level: u32,
    arena: Vec<u64>,
    trail: Vec<Trail>,
    queue: Vec<usize>,
    queued: Vec<bool>,
    rng: Rng,
    nodes: u64,
    budget: u64,
    score_weight: f32,
}

fn letters(mask: u32) -> impl Iterator<Item = Letter> {
    let mut m = mask;
    std::iter::from_fn(move || {
        (m != 0).then(|| {
            let l = m.trailing_zeros() as Letter;
            m &= m - 1;
            l
        })
    })
}

fn bits(set: &[u64]) -> impl Iterator<Item = u32> + '_ {
    set.iter().enumerate().flat_map(|(w, &x)| {
        let mut x = x;
        std::iter::from_fn(move || {
            (x != 0).then(|| {
                let b = x.trailing_zeros();
                x &= x - 1;
                (w * 64) as u32 + b
            })
        })
    })
}

impl<'d> Solver<'d> {
    /// Prépare le solveur et propage les contraintes initiales. Renvoie
    /// `None` si le gabarit est déjà insoluble (longueur absente du
    /// dictionnaire, mot imposé inconnu ou répété, croisement impossible...).
    pub(crate) fn new(dict: &'d Dictionary, grid: &Grid, rng: Rng, budget: u64) -> Option<Self> {
        let slots = grid.slots();
        let buckets = slots
            .iter()
            .map(|s| dict.bucket(s.len()))
            .collect::<Option<Vec<_>>>()?;
        let mut cell_slots = vec![[None, None]; grid.len()];
        let mut by_len = vec![Vec::new(); dict.max_len() + 1];
        for (si, s) in slots.iter().enumerate() {
            let dir = usize::from(!s.across);
            for (p, &c) in s.cells.iter().enumerate() {
                cell_slots[c][dir] = Some((si, p));
            }
            by_len[s.len()].push(si);
        }
        let dom = (0..grid.len())
            .map(|i| match grid.cell(i) {
                Cell::Open(Some(l)) => 1 << l,
                _ => ALL_LETTERS,
            })
            .collect();
        let cand = buckets
            .iter()
            .map(|b| {
                let mut v = vec![u64::MAX; b.nw];
                if let Some(last) = v.last_mut() {
                    *last = b.tail_mask();
                }
                v
            })
            .collect();
        let n = slots.len();
        let mut solver = Solver {
            grid: grid.clone(),
            count: buckets.iter().map(|b| b.count() as u32).collect(),
            buckets,
            slots,
            cell_slots,
            by_len,
            dom,
            cand,
            assigned: vec![None; n],
            saved_at: vec![0; n],
            level: 0,
            arena: Vec::new(),
            trail: Vec::new(),
            queue: Vec::new(),
            queued: vec![false; n],
            rng,
            nodes: 0,
            budget,
            score_weight: 1.0,
        };
        for si in 0..n {
            for p in 0..solver.slots[si].len() {
                let d = solver.dom[solver.slots[si].cells[p]];
                if d != ALL_LETTERS && !solver.filter(si, p, ALL_LETTERS, d) {
                    return None;
                }
            }
            solver.enqueue(si);
        }
        if !solver.propagate() {
            return None;
        }
        solver.trail.clear();
        solver.arena.clear();
        Some(solver)
    }

    /// Poids du score des mots (en log) dans l'ordre des candidats : positif,
    /// les mots fréquents passent d'abord ; négatif, les plus rares.
    pub(crate) fn with_score_weight(mut self, w: f32) -> Self {
        self.score_weight = w;
        self
    }

    fn enqueue(&mut self, s: usize) {
        if !self.queued[s] {
            self.queued[s] = true;
            self.queue.push(s);
        }
    }

    fn clear_queue(&mut self) {
        for s in self.queue.drain(..) {
            self.queued[s] = false;
        }
    }

    /// Sauvegarde les candidats de `s` une fois par niveau de décision.
    fn save(&mut self, s: usize) {
        if self.saved_at[s] != self.level {
            self.trail.push(Trail::Cand {
                slot: s,
                at: self.arena.len(),
                count: self.count[s],
                saved_at: self.saved_at[s],
            });
            self.arena.extend_from_slice(&self.cand[s]);
            self.saved_at[s] = self.level;
        }
    }

    fn set_dom(&mut self, c: usize, d: u32) {
        self.trail.push(Trail::Dom(c, self.dom[c]));
        self.dom[c] = d;
    }

    /// La case en position `p` de l'emplacement `s` passe des lettres `old` à
    /// `new` : retire les candidats devenus incompatibles. Les candidats
    /// respectent déjà `old`, il suffit donc de traiter la différence.
    /// Renvoie `false` s'il ne reste aucun candidat.
    fn filter(&mut self, s: usize, p: usize, old: u32, new: u32) -> bool {
        let b = self.buckets[s];
        let removed = old & !new;
        // On combine le plus petit des deux ensembles de lettres.
        let (mask, keep_mode) = if new.count_ones() <= removed.count_ones() {
            (new, true)
        } else {
            (removed, false)
        };
        let mut sl: [&[u64]; 26] = [&[]; 26];
        let mut k = 0;
        for l in letters(mask) {
            sl[k] = b.slice(p, l);
            k += 1;
        }
        let sl = &sl[..k];
        let mut n = 0;
        let mut saved = false;
        for w in 0..b.nw {
            let c = self.cand[s][w];
            if c == 0 {
                continue;
            }
            let m = sl.iter().fold(0, |m, x| m | x[w]);
            let nc = if keep_mode { c & m } else { c & !m };
            if nc != c {
                if !saved {
                    self.save(s);
                    saved = true;
                }
                self.cand[s][w] = nc;
            }
            n += nc.count_ones();
        }
        self.count[s] = n;
        n > 0
    }

    /// Propage jusqu'au point fixe. Renvoie `false` sur une impasse.
    fn propagate(&mut self) -> bool {
        let mut masks = Vec::new();
        while let Some(t) = self.queue.pop() {
            self.queued[t] = false;
            if self.count[t] == 0 {
                self.clear_queue();
                return false;
            }
            if self.count[t] > LETTER_CAP {
                continue;
            }
            let len = self.slots[t].len();
            let b = self.buckets[t];
            // Toutes les cases fixées : rien à retirer.
            if self.slots[t]
                .cells
                .iter()
                .all(|&c| self.dom[c].count_ones() == 1)
            {
                continue;
            }
            masks.clear();
            masks.resize(len, 0u32);
            for w in bits(&self.cand[t]) {
                for (m, &l) in masks.iter_mut().zip(b.word(w as usize)) {
                    *m |= 1 << l;
                }
            }
            let dir = usize::from(!self.slots[t].across);
            for (p, &m) in masks.iter().enumerate() {
                let c = self.slots[t].cells[p];
                let old = self.dom[c];
                let d = old & m;
                if d == old {
                    continue;
                }
                self.set_dom(c, d);
                if let Some((u, q)) = self.cell_slots[c][1 - dir] {
                    if !self.filter(u, q, old, d) {
                        self.clear_queue();
                        return false;
                    }
                    self.enqueue(u);
                }
            }
        }
        true
    }

    /// Place le mot `w` dans l'emplacement `s` et propage.
    fn assign(&mut self, s: usize, w: u32) -> bool {
        self.level += 1;
        self.save(s);
        self.cand[s].fill(0);
        self.cand[s][w as usize / 64] = 1 << (w % 64);
        self.count[s] = 1;
        self.assigned[s] = Some(w);
        self.trail.push(Trail::Assigned(s));
        let b = self.buckets[s];
        let dir = usize::from(!self.slots[s].across);
        for (p, &l) in b.word(w as usize).iter().enumerate() {
            let c = self.slots[s].cells[p];
            let old = self.dom[c];
            if old == 1 << l {
                continue;
            }
            self.set_dom(c, 1 << l);
            if let Some((u, q)) = self.cell_slots[c][1 - dir] {
                if !self.filter(u, q, old, 1 << l) {
                    self.clear_queue();
                    return false;
                }
                self.enqueue(u);
            }
        }
        let (word, bit) = (w as usize / 64, 1u64 << (w % 64));
        for k in 0..self.by_len[b.len].len() {
            let t = self.by_len[b.len][k];
            if t == s || self.assigned[t].is_some() || self.cand[t][word] & bit == 0 {
                continue;
            }
            self.save(t);
            self.cand[t][word] &= !bit;
            self.count[t] -= 1;
            if self.count[t] == 0 {
                self.clear_queue();
                return false;
            }
            self.enqueue(t);
        }
        self.propagate()
    }

    /// Retire le mot `w` des candidats de `s` (il vient d'être réfuté).
    fn refute(&mut self, s: usize, w: u32) -> bool {
        self.level += 1;
        self.save(s);
        self.cand[s][w as usize / 64] &= !(1 << (w % 64));
        self.count[s] -= 1;
        if self.count[s] == 0 {
            return false;
        }
        self.enqueue(s);
        self.propagate()
    }

    fn undo_to(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop().expect("piste non vide") {
                Trail::Dom(c, d) => self.dom[c] = d,
                Trail::Cand {
                    slot,
                    at,
                    count,
                    saved_at,
                } => {
                    let nw = self.cand[slot].len();
                    self.cand[slot].copy_from_slice(&self.arena[at..at + nw]);
                    self.arena.truncate(at);
                    self.count[slot] = count;
                    self.saved_at[slot] = saved_at;
                }
                Trail::Assigned(s) => self.assigned[s] = None,
            }
        }
    }

    /// Emplacement non attribué ayant le moins de candidats ; à égalité, le
    /// plus long, puis le premier rencontré à partir d'un point aléatoire.
    fn pick_slot(&mut self) -> Option<usize> {
        let n = self.slots.len();
        let start = self.rng.below(n.max(1));
        let mut best: Option<(u32, std::cmp::Reverse<usize>, usize)> = None;
        for k in 0..n {
            let si = (start + k) % n;
            if self.assigned[si].is_some() {
                continue;
            }
            let key = (self.count[si], std::cmp::Reverse(self.slots[si].len()), si);
            if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                best = Some(key);
            }
        }
        best.map(|b| b.2)
    }

    /// Candidats de `s`, le moins contraignant d'abord : on somme, sur les
    /// emplacements croisés, le log du nombre de mots qui restent compatibles
    /// avec la lettre imposée. Un mot qui vide un croisement est écarté. Un
    /// bruit aléatoire et le score du mot départagent.
    fn order_candidates(&mut self, s: usize) -> Vec<u32> {
        let b = self.buckets[s];
        let dir = usize::from(!self.slots[s].across);
        let mut weight = vec![[f32::NEG_INFINITY; 26]; b.len];
        for (p, row) in weight.iter_mut().enumerate() {
            let c = self.slots[s].cells[p];
            let d = self.dom[c];
            match self.cell_slots[c][1 - dir] {
                Some((t, q)) if self.assigned[t].is_none() => {
                    let tb = self.buckets[t];
                    for l in letters(d) {
                        let n: u32 = self.cand[t]
                            .iter()
                            .zip(tb.slice(q, l))
                            .map(|(&x, &y)| (x & y).count_ones())
                            .sum();
                        if n > 0 {
                            row[l as usize] = (n as f32).ln();
                        }
                    }
                }
                _ => {
                    for l in letters(d) {
                        row[l as usize] = 0.0;
                    }
                }
            }
        }
        let mut keyed: Vec<(f32, u32)> = bits(&self.cand[s])
            .filter_map(|w| {
                let fit: f32 = b
                    .word(w as usize)
                    .iter()
                    .zip(&weight)
                    .map(|(&l, row)| row[l as usize])
                    .sum();
                fit.is_finite().then_some((fit, w))
            })
            .collect();
        for (k, w) in &mut keyed {
            *k += self.score_weight * (b.scores[*w as usize] as f32 + 1.0).ln()
                + NOISE * self.rng.unit() as f32;
        }
        keyed.sort_by(|x, y| y.0.total_cmp(&x.0));
        keyed.into_iter().map(|(_, w)| w).collect()
    }

    fn search(&mut self) -> Outcome {
        let Some(s) = self.pick_slot() else {
            return Outcome::Solved;
        };
        let cands = self.order_candidates(s);
        let node_mark = self.trail.len();
        let mut outcome = Outcome::Impossible;
        for w in cands {
            // Un candidat a pu disparaître par propagation après un refus.
            if self.cand[s][w as usize / 64] & (1 << (w % 64)) == 0 {
                continue;
            }
            if self.nodes >= self.budget {
                outcome = Outcome::Budget;
                break;
            }
            self.nodes += 1;
            let mark = self.trail.len();
            if self.assign(s, w) {
                match self.search() {
                    Outcome::Impossible => {}
                    other => return other,
                }
            }
            self.undo_to(mark);
            if !self.refute(s, w) {
                break;
            }
        }
        self.undo_to(node_mark);
        outcome
    }

    /// Lance la recherche ; en cas de succès renvoie la grille remplie.
    pub(crate) fn run(mut self) -> (Outcome, Option<Solution>, u64) {
        let outcome = self.search();
        let sol = (outcome == Outcome::Solved).then(|| {
            let mut g = self.grid.clone();
            for (i, &d) in self.dom.iter().enumerate() {
                if !g.cell(i).is_block() && d.count_ones() == 1 {
                    g.set_cell(i, Cell::Open(Some(d.trailing_zeros() as Letter)));
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
        // 2×2 sans noire = 4 mots distincts nécessaires, deux disponibles.
        let dict = Dictionary::from_words(["AA", "AB"]);
        let grid = Grid::parse("..\n..").unwrap();
        match Solver::new(&dict, &grid, Rng::new(0), 1000) {
            None => {}
            Some(s) => {
                let (outcome, sol, _) = s.run();
                assert_eq!(outcome, Outcome::Impossible);
                assert!(sol.is_none());
            }
        }
    }

    #[test]
    fn search_proves_impossibility() {
        // Chaque emplacement a des candidats cohérents lettre à lettre, mais
        // il faudrait employer deux fois le même mot.
        let dict = Dictionary::from_words(["AB", "BA"]);
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
        for seed in 0..10 {
            let s = Solver::new(&dict, &grid, Rng::new(seed), 100_000).unwrap();
            let (outcome, sol, _) = s.run();
            assert_eq!(outcome, Outcome::Solved);
            let sol = sol.unwrap();
            let mut seen = std::collections::HashSet::new();
            for e in &sol.entries {
                assert!(dict.contains(&e.word), "{} absent du dictionnaire", e.word);
                assert!(seen.insert(e.word.clone()), "{} répété", e.word);
            }
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

    #[test]
    fn repeated_imposed_word_is_rejected() {
        let dict = Dictionary::from_words(["AB", "CD"]);
        let grid = Grid::parse("AB\n##\nAB").unwrap();
        match Solver::new(&dict, &grid, Rng::new(0), 100) {
            None => {}
            Some(s) => assert_eq!(s.run().0, Outcome::Impossible),
        }
    }
}
