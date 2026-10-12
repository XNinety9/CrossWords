//! # motscroises
//!
//! Génération de grilles de mots croisés « à la française » et de mots
//! fléchés : grille rectangulaire avec cases noires (ou cases définitions),
//! chaque case blanche appartenant à un mot horizontal et/ou vertical, mots
//! tous issus d'un dictionnaire fourni.
//!
//! Mots croisés :
//!
//! - [`fill`] remplit un gabarit donné (cases noires placées, lettres
//!   éventuellement imposées) ;
//! - [`generate`] tire des gabarits au hasard et les remplit jusqu'à succès.
//!
//! Mots fléchés : [`generate_fleches`] et [`fill_fleches`], qui rattachent en
//! plus chaque mot à sa case définition et à sa flèche (voir [`Fleches`]).
//!
//! Le remplissage est un problème de satisfaction de contraintes : lettres
//! possibles par case et mots candidats par emplacement (bitsets, voir
//! [`Dictionary`]), propagés jusqu'au point fixe ; recherche en profondeur
//! sur l'emplacement le plus contraint, mot le moins contraignant d'abord ;
//! budget de nœuds court et nouveaux tirages plutôt qu'une recherche
//! acharnée. Aucune dépendance, et un résultat entièrement déterminé par la
//! graine : même dictionnaire, mêmes options, même grille.
//!
//! Sans dictionnaire sous la main, [`wordlist::french`] télécharge (une seule
//! fois) une liste française avec fréquences d'usage.
//!
//! ```
//! use motscroises::{Dictionary, Grid, FillOptions, fill};
//!
//! let dict = Dictionary::from_words(["ABC", "DEF", "GHI", "ADG", "BEH", "CFI"]);
//! let grid = Grid::parse("...\n...\n...").unwrap();
//! let sol = fill(&dict, &grid, &FillOptions::default()).unwrap();
//! assert_eq!(sol.entries.len(), 6);
//! ```

pub mod autodefs;
mod dict;
mod fleches;
mod grid;
mod json;
mod pattern;
mod rng;
mod solver;
pub mod wordlist;

pub use dict::{Dictionary, Letter, letter_char, normalize};
pub use fleches::{
    Arrow, Clue, Definitions, Fleches, FlechesOptions, LEVELS, LevelSettings, assign_clues,
    default_inner_ratio, fill_fleches, generate_fleches, is_valid_fleches_pattern, level_settings,
    random_fleches_pattern,
};
pub use grid::{Cell, Entry, Grid, ParseError, Slot, Solution, roman};
pub use pattern::{PatternOptions, default_ratio, random_pattern};
pub use rng::Rng;

use solver::{Outcome, Solver};
use std::fmt;

/// Erreurs de remplissage ou de génération.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Le gabarit ne peut pas être rempli avec ce dictionnaire (prouvé par
    /// exploration complète, ou constaté d'emblée).
    NoSolution,
    /// Aucune solution trouvée dans le budget alloué ; rien n'est prouvé.
    GaveUp { attempts: u32, nodes: u64 },
    /// Le gabarit contient des cases blanches n'appartenant à aucun mot.
    IsolatedCells(Vec<(usize, usize)>),
    /// Options incohérentes.
    InvalidOptions(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoSolution => write!(f, "aucune solution avec ce dictionnaire"),
            Error::GaveUp { attempts, nodes } => {
                write!(
                    f,
                    "abandon après {attempts} tentatives ({nodes} nœuds explorés)"
                )
            }
            Error::IsolatedCells(c) => {
                let list: Vec<String> = c
                    .iter()
                    .map(|(r, c)| format!("{} {}", roman(r + 1), c + 1))
                    .collect();
                write!(f, "cases blanches hors de tout mot : {}", list.join(", "))
            }
            Error::InvalidOptions(m) => write!(f, "options invalides : {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Options de remplissage d'un gabarit.
#[derive(Debug, Clone)]
pub struct FillOptions {
    pub seed: u64,
    /// Nœuds alloués à la première tentative ; chaque redémarrage multiplie
    /// ce budget par 1,5.
    pub node_budget: u64,
    /// Nombre de tentatives (redémarrages aléatoires).
    pub attempts: u32,
    /// Préférence pour les mots de bon score : positif, les mots courants
    /// d'abord ; nul, indifférent ; négatif, les plus rares d'abord.
    pub score_weight: f32,
}

impl Default for FillOptions {
    fn default() -> Self {
        FillOptions {
            seed: 0,
            node_budget: 300,
            attempts: 15,
            score_weight: 1.0,
        }
    }
}

/// Remplit un gabarit.
pub fn fill(dict: &Dictionary, grid: &Grid, opts: &FillOptions) -> Result<Solution, Error> {
    let isolated = grid.isolated_cells();
    if !isolated.is_empty() {
        return Err(Error::IsolatedCells(isolated));
    }
    let mut rng = Rng::new(opts.seed);
    let mut budget = opts.node_budget.max(1) as f64;
    let mut total = 0;
    for _ in 0..opts.attempts.max(1) {
        let attempt_rng = Rng::new(rng.next_u64());
        let solver = Solver::new(dict, grid, attempt_rng, budget as u64)
            .ok_or(Error::NoSolution)?
            .with_score_weight(opts.score_weight);
        let (outcome, sol, nodes) = solver.run();
        total += nodes;
        match outcome {
            Outcome::Solved => return Ok(sol.expect("solution présente")),
            Outcome::Impossible => return Err(Error::NoSolution),
            Outcome::Budget => budget *= 1.5,
        }
    }
    Err(Error::GaveUp {
        attempts: opts.attempts.max(1),
        nodes: total,
    })
}

/// Options de génération complète (gabarit + remplissage).
#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub pattern: PatternOptions,
    pub seed: u64,
    /// Nombre de gabarits essayés.
    pub attempts: u32,
    /// Nœuds alloués au remplissage de chaque gabarit.
    pub node_budget: u64,
    /// Voir [`FillOptions::score_weight`].
    pub score_weight: f32,
}

impl GenerateOptions {
    pub fn new(width: usize, height: usize) -> Self {
        GenerateOptions {
            pattern: PatternOptions::new(width, height),
            seed: 0,
            attempts: 1_000,
            node_budget: 200,
            score_weight: 1.0,
        }
    }
}

/// Statistiques d'une génération réussie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Numéro (à partir de 1) du gabarit qui a pu être rempli.
    pub attempt: u32,
    /// Nœuds explorés au total, toutes tentatives confondues.
    pub nodes: u64,
}

/// Tire des gabarits et tente de les remplir jusqu'au premier succès.
pub fn generate(dict: &Dictionary, opts: &GenerateOptions) -> Result<(Solution, Stats), Error> {
    let p = &opts.pattern;
    if p.width < 2 || p.height < 2 {
        return Err(Error::InvalidOptions(
            "la grille doit faire au moins 2×2".into(),
        ));
    }
    if !(0.0..0.5).contains(&p.black_ratio) {
        return Err(Error::InvalidOptions(
            "le taux de noires doit être dans [0 ; 0,5[".into(),
        ));
    }
    if p.max_word_len > dict.max_len() {
        let mut p2 = p.clone();
        p2.max_word_len = dict.max_len();
        let mut o2 = opts.clone();
        o2.pattern = p2;
        return generate(dict, &o2);
    }
    let mut rng = Rng::new(opts.seed);
    let mut total = 0;
    for attempt in 1..=opts.attempts.max(1) {
        let mut prng = Rng::new(rng.next_u64());
        let Some(grid) = random_pattern(p, &mut prng) else {
            continue;
        };
        let Some(solver) = Solver::new(dict, &grid, Rng::new(rng.next_u64()), opts.node_budget)
        else {
            continue;
        };
        let solver = solver.with_score_weight(opts.score_weight);
        let (outcome, sol, nodes) = solver.run();
        total += nodes;
        if outcome == Outcome::Solved {
            return Ok((
                sol.expect("solution présente"),
                Stats {
                    attempt,
                    nodes: total,
                },
            ));
        }
    }
    Err(Error::GaveUp {
        attempts: opts.attempts.max(1),
        nodes: total,
    })
}
