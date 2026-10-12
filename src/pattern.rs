//! Génération de gabarits « à la française » : pas de symétrie imposée,
//! cases noires plutôt isolées, placées en priorité là où elles coupent les
//! plus longues suites de cases blanches.

use crate::grid::{Cell, Grid};
use crate::rng::Rng;

/// Contraintes de placement des cases noires.
#[derive(Debug, Clone)]
pub struct PatternOptions {
    pub width: usize,
    pub height: usize,
    /// Proportion visée de cases noires (0.0 à 0.4 raisonnable).
    pub black_ratio: f64,
    /// Autorise deux cases noires côte à côte (horizontalement ou verticalement).
    pub allow_adjacent: bool,
    /// Longueur maximale d'un mot ; des noires supplémentaires sont ajoutées
    /// tant qu'une suite blanche la dépasse.
    pub max_word_len: usize,
    /// Nombre maximal de cases « muettes » dans un sens (encadrées par deux
    /// noires ou une noire et un bord, donc hors de tout mot dans ce sens).
    pub max_unchecked: usize,
}

impl PatternOptions {
    pub fn new(width: usize, height: usize) -> Self {
        PatternOptions {
            width,
            height,
            black_ratio: default_ratio(width, height),
            allow_adjacent: false,
            max_word_len: width.max(height),
            max_unchecked: (width * height) / 12,
        }
    }
}

/// Taux de noires par défaut, d'autant plus élevé que la grille est grande
/// (les longs mots qui se croisent beaucoup sont durs à remplir).
pub fn default_ratio(width: usize, height: usize) -> f64 {
    let side = ((width * height) as f64).sqrt();
    (0.10 + (side - 6.0) * 0.012).clamp(0.08, 0.20)
}

fn run_len(g: &Grid, i: usize, across: bool) -> usize {
    let (w, h) = (g.width(), g.height());
    let (r, c) = (i / w, i % w);
    let (pos, lim) = if across { (c, w) } else { (r, h) };
    let at = |k: usize| if across { r * w + k } else { k * w + c };
    if g.cell(at(pos)).is_block() {
        return 0;
    }
    let mut a = pos;
    while a > 0 && !g.cell(at(a - 1)).is_block() {
        a -= 1;
    }
    let mut b = pos;
    while b + 1 < lim && !g.cell(at(b + 1)).is_block() {
        b += 1;
    }
    b - a + 1
}

fn unchecked_count(g: &Grid) -> usize {
    (0..g.len())
        .filter(|&i| !g.cell(i).is_block())
        .map(|i| usize::from(run_len(g, i, true) == 1) + usize::from(run_len(g, i, false) == 1))
        .sum()
}

/// Essaie de noircir la case `i` ; défait et renvoie `false` si une
/// contrainte dure est violée.
fn try_block(g: &mut Grid, i: usize, opts: &PatternOptions) -> bool {
    if g.cell(i).is_block() {
        return false;
    }
    if !opts.allow_adjacent && g.neighbors(i).any(|j| g.cell(j).is_block()) {
        return false;
    }
    g.set_cell(i, Cell::Block);
    let neighbors: Vec<usize> = g.neighbors(i).collect();
    let ok = neighbors
        .iter()
        .all(|&j| g.cell(j).is_block() || run_len(g, j, true) > 1 || run_len(g, j, false) > 1)
        && g.is_connected();
    if !ok {
        g.set_cell(i, Cell::Open(None));
    }
    ok
}

/// Tire un gabarit au hasard. Renvoie `None` si les contraintes n'ont pas pu
/// être satisfaites après plusieurs essais (taux trop élevé pour la taille,
/// longueur maximale trop courte, par exemple).
pub fn random_pattern(opts: &PatternOptions, rng: &mut Rng) -> Option<Grid> {
    (0..64).find_map(|_| try_pattern(opts, rng))
}

fn try_pattern(opts: &PatternOptions, rng: &mut Rng) -> Option<Grid> {
    let mut g = Grid::new(opts.width, opts.height);
    let n = g.len();
    let target = (n as f64 * opts.black_ratio).round() as usize;
    let mut placed = 0;
    let mut rejected = vec![false; n];
    loop {
        let too_long = (0..n).any(|i| {
            run_len(&g, i, true) > opts.max_word_len || run_len(&g, i, false) > opts.max_word_len
        });
        if placed >= target && !too_long {
            break;
        }
        // Poids : somme des carrés des longueurs de suites traversant la case,
        // pour couper d'abord les longues suites.
        let base_unchecked = unchecked_count(&g);
        let weights: Vec<f64> = (0..n)
            .map(|i| {
                if rejected[i] || g.cell(i).is_block() {
                    return 0.0;
                }
                let h = run_len(&g, i, true) as f64;
                let v = run_len(&g, i, false) as f64;
                let mut w = h * h + v * v;
                if too_long && h as usize <= opts.max_word_len && v as usize <= opts.max_word_len {
                    // Phase de correction : seules comptent les cases qui
                    // coupent une suite trop longue.
                    w = 0.0;
                }
                w
            })
            .collect();
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = rng.unit() * total;
        let mut pick = n - 1;
        for (i, &w) in weights.iter().enumerate() {
            if x < w {
                pick = i;
                break;
            }
            x -= w;
        }
        if try_block(&mut g, pick, opts) {
            let added = unchecked_count(&g).saturating_sub(base_unchecked);
            // Le plafond de cases muettes ne s'applique qu'hors phase de
            // correction : couper une suite trop longue prime.
            if !too_long && unchecked_count(&g) > opts.max_unchecked && added > 0 {
                g.set_cell(pick, Cell::Open(None));
                rejected[pick] = true;
                continue;
            }
            placed += 1;
            rejected.iter_mut().for_each(|r| *r = false);
        } else {
            rejected[pick] = true;
        }
    }
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invariants() {
        for seed in 0..50 {
            let mut rng = Rng::new(seed);
            let opts = PatternOptions::new(10, 10);
            let g = random_pattern(&opts, &mut rng).expect("gabarit");
            assert!(g.is_connected());
            assert!(g.isolated_cells().is_empty());
            assert!(g.block_count() >= (100.0 * opts.black_ratio).round() as usize);
            for i in 0..g.len() {
                if g.cell(i).is_block() {
                    assert!(
                        g.neighbors(i).all(|j| !g.cell(j).is_block()),
                        "noires adjacentes"
                    );
                }
            }
            assert!(g.slots().iter().all(|s| s.len() <= opts.max_word_len));
        }
    }

    #[test]
    fn max_word_len_is_enforced() {
        let mut opts = PatternOptions::new(9, 9);
        opts.max_word_len = 6;
        let g = random_pattern(&opts, &mut Rng::new(5)).unwrap();
        assert!(g.slots().iter().all(|s| s.len() <= 6));
    }

    #[test]
    fn deterministic() {
        let opts = PatternOptions::new(11, 9);
        let a = random_pattern(&opts, &mut Rng::new(77));
        let b = random_pattern(&opts, &mut Rng::new(77));
        assert_eq!(a, b);
    }
}
