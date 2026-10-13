//! Mots fléchés : les cases noires deviennent des cases définitions. Chacune
//! porte une ou deux définitions, dont la flèche pointe vers la première
//! lettre du mot :
//!
//! - flèche droite `→` (mot horizontal commençant juste à droite) ;
//! - flèche droite `↓` (mot vertical commençant juste en dessous) ;
//! - flèche coudée `↴` (vers la case de droite, puis vers le bas) : mot
//!   vertical qui commence sur la première rangée ;
//! - flèche coudée `↳` (vers la case du dessous, puis vers la droite) : mot
//!   horizontal qui commence dans la première colonne.
//!
//! Le gabarit classique alterne cases définitions et cases lettres sur la
//! première rangée et la première colonne, puis des cases définitions sont
//! semées à l'intérieur pour couper les mots.

use crate::dict::{Dictionary, Letter, letter_char, normalize};
use crate::grid::{Cell, Grid, Solution, roman};
use crate::rng::Rng;
use crate::solver::{Outcome, Solver};
use crate::{Error, Language};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, BufRead};

/// Sens de la flèche d'une définition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Arrow {
    /// Mot horizontal commençant à droite de la case définition.
    Right,
    /// Flèche coudée : case de droite, puis mot vertical vers le bas.
    RightDown,
    /// Mot vertical commençant sous la case définition.
    Down,
    /// Flèche coudée : case du dessous, puis mot horizontal vers la droite.
    DownRight,
}

impl Arrow {
    pub fn symbol(self) -> char {
        match self {
            Arrow::Right => '→',
            Arrow::RightDown => '↴',
            Arrow::Down => '↓',
            Arrow::DownRight => '↳',
        }
    }
}

/// Une définition placée : case définition, flèche, et index du mot dans
/// `Solution::entries`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clue {
    pub row: usize,
    pub col: usize,
    pub arrow: Arrow,
    pub entry: usize,
}

/// Rattache chaque mot du gabarit à une case définition.
///
/// Un mot qui ne commence pas sur un bord est précédé d'une case définition
/// (flèche droite). Un mot qui commence sur le bord gauche (resp. haut) doit
/// être défini depuis la case du dessus (resp. de gauche) par une flèche
/// coudée. Une case porte au plus deux définitions. Avec `strict`, toute case
/// définition doit servir au moins une fois.
///
/// Les définitions sont renvoyées dans l'ordre des emplacements de
/// [`Grid::slots`], qui est aussi celui de `Solution::entries`.
pub fn assign_clues(grid: &Grid, strict: bool) -> Result<Vec<Clue>, String> {
    let w = grid.width();
    let mut per_cell: HashMap<(usize, usize), usize> = HashMap::new();
    let mut clues = Vec::new();
    for (i, s) in grid.slots().iter().enumerate() {
        let (r, c) = (s.row, s.col);
        let (dr, dc, arrow) = match (s.across, r, c) {
            (true, _, c) if c > 0 => (r, c - 1, Arrow::Right),
            (true, r, 0) if r > 0 && grid.get(r - 1, 0).is_block() => (r - 1, 0, Arrow::DownRight),
            (false, r, _) if r > 0 => (r - 1, c, Arrow::Down),
            (false, 0, c) if c > 0 && grid.get(0, c - 1).is_block() => (0, c - 1, Arrow::RightDown),
            _ => {
                return Err(format!(
                    "le mot {} en {} {} n'a pas de case définition",
                    if s.across { "horizontal" } else { "vertical" },
                    roman(r + 1),
                    c + 1
                ));
            }
        };
        let n = per_cell.entry((dr, dc)).or_insert(0);
        *n += 1;
        if *n > 2 {
            return Err(format!(
                "plus de deux définitions en {} {}",
                roman(dr + 1),
                dc + 1
            ));
        }
        clues.push(Clue {
            row: dr,
            col: dc,
            arrow,
            entry: i,
        });
    }
    if strict {
        for i in 0..grid.len() {
            let (r, c) = (i / w, i % w);
            if grid.get(r, c).is_block() && !per_cell.contains_key(&(r, c)) {
                return Err(format!(
                    "case définition sans définition en {} {}",
                    roman(r + 1),
                    c + 1
                ));
            }
        }
    }
    Ok(clues)
}

/// Niveaux de difficulté des mots fléchés, du plus facile au plus difficile.
pub const LEVELS: std::ops::RangeInclusive<u8> = 1..=4;

/// Définitions à afficher : `MOT<TAB>définition`, une par ligne, suivie
/// éventuellement d'une tabulation et d'un niveau de difficulté (1 à 4).
/// Sans niveau, la définition vaut pour tous les niveaux. Plusieurs lignes
/// pour un même mot donnent plusieurs définitions possibles.
#[derive(Debug, Default, Clone)]
pub struct Definitions {
    /// Définitions par mot, avec leur niveau (0 : tous niveaux).
    map: HashMap<Vec<Letter>, Vec<(String, u8)>>,
}

impl Definitions {
    pub fn from_reader<R: BufRead>(reader: R) -> io::Result<Self> {
        let mut d = Definitions::default();
        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((word, rest)) = line.split_once(['\t', ';']) else {
                continue;
            };
            let (def, level) = match rest.rsplit_once('\t') {
                Some((def, l)) => match l.trim().parse::<u8>() {
                    Ok(n) if LEVELS.contains(&n) => (def, n),
                    _ => (rest, 0),
                },
                None => (rest, 0),
            };
            d.insert_level(word.trim(), def.trim(), level);
        }
        Ok(d)
    }

    /// Ajoute une définition valable pour tous les niveaux.
    pub fn insert(&mut self, word: &str, definition: &str) {
        self.insert_level(word, definition, 0);
    }

    /// Ajoute une définition d'un niveau donné (0 : tous niveaux).
    pub fn insert_level(&mut self, word: &str, definition: &str, level: u8) {
        if let Some(n) = normalize(word) {
            self.map
                .entry(n)
                .or_default()
                .push((definition.to_string(), level));
        }
    }

    /// Définitions retenues pour un niveau : pour chaque mot, celles de ce
    /// niveau, sinon celles valables pour tous les niveaux.
    pub fn at_level(&self, level: u8) -> Definitions {
        let map = self
            .map
            .iter()
            .filter_map(|(k, v)| {
                let exact: Vec<_> = v.iter().filter(|(_, l)| *l == level).cloned().collect();
                let kept = if exact.is_empty() {
                    v.iter().filter(|(_, l)| *l == 0).cloned().collect()
                } else {
                    exact
                };
                (!kept.is_empty()).then(|| (k.clone(), kept))
            })
            .collect();
        Definitions { map }
    }

    /// Une définition du mot, choisie de façon déterministe s'il y en a plusieurs.
    pub fn get(&self, word: &str) -> Option<&str> {
        let n = normalize(word)?;
        let v = self.map.get(&n)?;
        let h = n
            .iter()
            .fold(0usize, |h, &l| h.wrapping_mul(31).wrapping_add(l as usize));
        v.get(h % v.len()).map(|(d, _)| d.as_str())
    }

    /// Ajoute les mots de `other` absents d'ici : les définitions déjà
    /// présentes priment.
    pub fn extend_missing(&mut self, other: Definitions) {
        for (k, v) in other.map {
            self.map.entry(k).or_insert(v);
        }
    }

    /// Couples (mot, définition) triés par mot, une définition par mot.
    pub fn entries(&self) -> Vec<(String, &str)> {
        let mut v: Vec<(String, &str)> = self
            .map
            .iter()
            .filter_map(|(k, defs)| {
                let w = k.iter().map(|&l| letter_char(l)).collect();
                defs.first().map(|(d, _)| (w, d.as_str()))
            })
            .collect();
        v.sort();
        v
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Grille de mots fléchés remplie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fleches {
    /// Grille et mots ; les cases `Block` sont les cases définitions.
    pub solution: Solution,
    pub clues: Vec<Clue>,
}

impl Fleches {
    pub(crate) fn from_solution(solution: Solution, strict: bool) -> Result<Self, String> {
        let clues = assign_clues(&solution.grid, strict)?;
        Ok(Fleches { solution, clues })
    }

    /// Définitions d'une case, ordonnées de haut en bas.
    pub fn clues_at(&self, row: usize, col: usize) -> Vec<&Clue> {
        let mut v: Vec<&Clue> = self
            .clues
            .iter()
            .filter(|c| c.row == row && c.col == col)
            .collect();
        v.sort_by_key(|c| c.arrow);
        v
    }

    fn clue_text<'a>(&'a self, clue: &Clue, defs: Option<&'a Definitions>) -> String {
        let word = &self.solution.entries[clue.entry].word;
        match defs.and_then(|d| d.get(word)) {
            Some(d) => d.to_string(),
            None => format!("({})", word.len()),
        }
    }

    /// Liste des définitions, case par case en ordre de lecture.
    pub fn clue_list(&self, defs: Option<&Definitions>) -> String {
        let g = &self.solution.grid;
        let mut s = String::new();
        for r in 0..g.height() {
            for c in 0..g.width() {
                for clue in self.clues_at(r, c) {
                    let word = &self.solution.entries[clue.entry].word;
                    let def = defs.and_then(|d| d.get(word)).unwrap_or("");
                    let _ = writeln!(
                        s,
                        "  {:>5} {:<2} {}  {:<16} {}",
                        roman(r + 1),
                        c + 1,
                        clue.arrow.symbol(),
                        word,
                        def
                    );
                }
            }
        }
        s
    }

    /// Définitions numérotées en ordre de lecture : case par case, puis de
    /// haut en bas dans une case.
    pub fn numbered_clues(&self) -> Vec<(usize, &Clue)> {
        let g = &self.solution.grid;
        let mut v = Vec::new();
        for r in 0..g.height() {
            for c in 0..g.width() {
                for clue in self.clues_at(r, c) {
                    v.push((v.len() + 1, clue));
                }
            }
        }
        v
    }

    /// Liste numérotée des définitions, comme sous une grille à numéros.
    pub fn numbered_clue_list(&self, defs: Option<&Definitions>) -> String {
        let mut s = String::new();
        for (n, clue) in self.numbered_clues() {
            let word = &self.solution.entries[clue.entry].word;
            let def = defs.and_then(|d| d.get(word)).unwrap_or("");
            let _ = writeln!(s, "  {n:>3}. {}  {:<16} {}", clue.arrow.symbol(), word, def);
        }
        s
    }

    /// Mots qui n'ont pas de définition dans `defs`, triés et sans doublon :
    /// pratique pour savoir quoi rédiger.
    pub fn missing_definitions(&self, defs: Option<&Definitions>) -> Vec<String> {
        let mut v: Vec<String> = self
            .solution
            .entries
            .iter()
            .filter(|e| defs.and_then(|d| d.get(&e.word)).is_none())
            .map(|e| e.word.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Rendu SVG, définitions dans les cases. Sans définition connue, la case
    /// affiche la longueur du mot.
    pub fn to_svg(&self, with_letters: bool, defs: Option<&Definitions>) -> String {
        self.render(with_letters, defs, None)
    }

    /// Rendu SVG à numéros : les cases définitions portent un numéro et une
    /// flèche, les définitions numérotées sont listées sous la grille.
    pub fn to_svg_numbered(&self, with_letters: bool, defs: Option<&Definitions>) -> String {
        self.render(with_letters, defs, Some(Language::Fr))
    }

    /// Comme [`Fleches::to_svg_numbered`], avec le titre de la liste dans la
    /// langue voulue (« Définitions » ou « Clues »).
    pub fn to_svg_numbered_in(
        &self,
        with_letters: bool,
        defs: Option<&Definitions>,
        lang: Language,
    ) -> String {
        self.render(with_letters, defs, Some(lang))
    }

    /// `numbered` : `None` pour les définitions dans les cases, sinon la
    /// langue de la liste numérotée sous la grille.
    fn render(
        &self,
        with_letters: bool,
        defs: Option<&Definitions>,
        numbered: Option<Language>,
    ) -> String {
        let title = match numbered {
            Some(Language::En) => "Clues",
            _ => "Définitions",
        };
        let numbered = numbered.is_some();
        const C: usize = 64;
        const P: usize = 4;
        let g = &self.solution.grid;
        let numbers: HashMap<(usize, usize, Arrow), usize> = self
            .numbered_clues()
            .into_iter()
            .map(|(n, c)| ((c.row, c.col, c.arrow), n))
            .collect();
        let grid_h = g.height() * C;
        let list = numbered.then(|| self.clue_columns(defs, g.width() * C));
        let list_h = list.as_ref().map_or(0, |l| l.height);
        let (w, h) = (P * 2 + g.width() * C, P * 2 + grid_h + list_h);
        let mut s = String::new();
        let _ = write!(
            s,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" font-family="Helvetica, Arial, sans-serif">"#
        );
        s.push_str(r#"<rect width="100%" height="100%" fill="white"/>"#);
        // Les flèches débordent sur les cases voisines : on les dessine après
        // toutes les cases pour qu'elles ne soient pas recouvertes.
        let mut arrows = String::new();
        // Cases
        for r in 0..g.height() {
            for c in 0..g.width() {
                let (x, y) = (P + c * C, P + r * C);
                let cell = g.get(r, c);
                let fill = if cell.is_block() { "#d9e4ee" } else { "white" };
                let _ = write!(
                    s,
                    r##"<rect x="{x}" y="{y}" width="{C}" height="{C}" fill="{fill}" stroke="#222" stroke-width="1"/>"##
                );
                match cell {
                    Cell::Open(Some(l)) if with_letters => {
                        let _ = write!(
                            s,
                            r#"<text x="{}" y="{}" font-size="30" text-anchor="middle" font-weight="bold">{}</text>"#,
                            x + C / 2,
                            y + C / 2 + 11,
                            letter_char(l)
                        );
                    }
                    Cell::Block => {
                        let clues = self.clues_at(r, c);
                        if clues.is_empty() {
                            let _ = write!(
                                s,
                                r##"<rect x="{x}" y="{y}" width="{C}" height="{C}" fill="#222"/>"##
                            );
                        }
                        let n = clues.len();
                        let part = C / n.max(1);
                        if n == 2 {
                            let _ = write!(
                                s,
                                r##"<line x1="{x}" y1="{}" x2="{}" y2="{}" stroke="#222" stroke-width="1"/>"##,
                                y + part,
                                x + C,
                                y + part
                            );
                        }
                        for (k, clue) in clues.iter().enumerate() {
                            let top = y + k * part;
                            if numbered {
                                let size = if n == 2 { 14 } else { 18 };
                                let _ = write!(
                                    s,
                                    r#"<text x="{}" y="{}" font-size="{size}" font-weight="bold" text-anchor="middle">{}</text>"#,
                                    x + C / 2,
                                    top + part / 2 + size / 3,
                                    numbers[&(clue.row, clue.col, clue.arrow)]
                                );
                            } else {
                                let text = self.clue_text(clue, defs);
                                let lines = wrap(&text, 11, if n == 2 { 3 } else { 6 });
                                let lh = 9;
                                let y0 = top + part / 2 + lh / 2 - (lines.len() * lh) / 2 + 2;
                                for (j, line) in lines.iter().enumerate() {
                                    let _ = write!(
                                        s,
                                        r#"<text x="{}" y="{}" font-size="8.5" text-anchor="middle">{}</text>"#,
                                        x + C / 2,
                                        y0 + j * lh,
                                        xml_escape(line)
                                    );
                                }
                            }
                            let shared_bottom = clue.arrow == Arrow::Down
                                && clues.iter().any(|o| o.arrow == Arrow::DownRight);
                            arrows.push_str(&arrow_svg(
                                clue.arrow,
                                x,
                                y,
                                C,
                                top + part / 2,
                                shared_bottom,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        let _ = write!(
            s,
            r##"<rect x="{P}" y="{P}" width="{}" height="{}" fill="none" stroke="#222" stroke-width="2.5"/>"##,
            g.width() * C,
            g.height() * C
        );
        s.push_str(&arrows);
        if let Some(list) = list {
            let y0 = P + grid_h;
            let _ = write!(
                s,
                r#"<text x="{P}" y="{}" font-size="15" font-weight="bold">{title}</text>"#,
                y0 + LIST_TITLE - 8
            );
            for e in &list.entries {
                let (x, y) = (P + e.column * list.column_width, y0 + LIST_TITLE + e.y);
                let _ = write!(
                    s,
                    r#"<text x="{}" y="{}" font-size="{LIST_FONT}" font-weight="bold" text-anchor="end">{}</text>"#,
                    x + LIST_NUM_W - 8,
                    y + LIST_LH,
                    e.number
                );
                for (j, line) in e.lines.iter().enumerate() {
                    let _ = write!(
                        s,
                        r#"<text x="{}" y="{}" font-size="{LIST_FONT}">{}</text>"#,
                        x + LIST_NUM_W,
                        y + LIST_LH * (j + 1),
                        xml_escape(line)
                    );
                }
            }
        }
        s.push_str("</svg>\n");
        s
    }

    /// Mise en page des définitions numérotées sur deux colonnes, la
    /// première remplie d'abord.
    fn clue_columns(&self, defs: Option<&Definitions>, width: usize) -> ClueColumns {
        let column_width = width / 2;
        // Environ 0,5 em par caractère en Helvetica.
        let chars = (column_width - LIST_NUM_W - 12) * 2 / LIST_FONT;
        let mut entries: Vec<ListEntry> = self
            .numbered_clues()
            .into_iter()
            .map(|(number, clue)| ListEntry {
                number,
                lines: wrap(&self.clue_text(clue, defs), chars, 4),
                column: 0,
                y: 0,
            })
            .collect();
        let block = |e: &ListEntry| e.lines.len() * LIST_LH + LIST_GAP;
        let total: usize = entries.iter().map(block).sum();
        let (mut y, mut column, mut height) = (0, 0, 0);
        for e in &mut entries {
            if column == 0 && y > 0 && y + block(e) / 2 > total.div_ceil(2) {
                column = 1;
                y = 0;
            }
            e.column = column;
            e.y = y;
            y += block(e);
            height = height.max(y);
        }
        ClueColumns {
            entries,
            column_width,
            height: LIST_TITLE + height + LIST_GAP,
        }
    }
}

const LIST_FONT: usize = 13;
const LIST_LH: usize = 17;
const LIST_GAP: usize = 5;
const LIST_NUM_W: usize = 36;
const LIST_TITLE: usize = 34;

struct ListEntry {
    number: usize,
    lines: Vec<String>,
    column: usize,
    /// Décalage vertical dans la colonne.
    y: usize,
}

struct ClueColumns {
    entries: Vec<ListEntry>,
    column_width: usize,
    height: usize,
}

/// Petite flèche dessinée dans la case visée, contre le bord de la case définition.
fn arrow_svg(
    arrow: Arrow,
    x: usize,
    y: usize,
    c: usize,
    mid_y: usize,
    shared_bottom: bool,
) -> String {
    let (x, y, c, mid_y) = (x as f64, y as f64, c as f64, mid_y as f64);
    let t = 5.0; // demi-largeur du triangle
    let style = r##"fill="#222""##;
    let line = r##"stroke="#222" stroke-width="1.4" fill="none""##;
    match arrow {
        Arrow::Right => {
            let bx = x + c;
            format!(
                r#"<path d="M{bx} {} L{} {mid_y} L{bx} {} Z" {style}/>"#,
                mid_y - t,
                bx + t * 1.4,
                mid_y + t
            )
        }
        Arrow::Down => {
            // Si une flèche coudée partage le bord bas, on décale vers la gauche.
            let bx = x + if shared_bottom { c * 0.3 } else { c * 0.5 };
            let by = y + c;
            format!(
                r#"<path d="M{} {by} L{bx} {} L{} {by} Z" {style}/>"#,
                bx - t,
                by + t * 1.4,
                bx + t
            )
        }
        Arrow::RightDown => {
            let bx = x + c;
            let ex = bx + 8.0;
            format!(
                r#"<path d="M{bx} {mid_y} L{ex} {mid_y} L{ex} {}" {line}/><path d="M{} {} L{ex} {} L{} {} Z" {style}/>"#,
                mid_y + 6.0,
                ex - t,
                mid_y + 6.0,
                mid_y + 6.0 + t * 1.4,
                ex + t,
                mid_y + 6.0
            )
        }
        Arrow::DownRight => {
            let bx = x + c * 0.7;
            let by = y + c;
            let ey = by + 8.0;
            format!(
                r#"<path d="M{bx} {by} L{bx} {ey} L{} {ey}" {line}/><path d="M{} {} L{} {ey} L{} {} Z" {style}/>"#,
                bx + 6.0,
                bx + 6.0,
                ey - t,
                bx + 6.0 + t * 1.4,
                bx + 6.0,
                ey + t
            )
        }
    }
}

fn wrap(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(l) = lines.last_mut() {
            l.push('…');
        }
    }
    lines
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Options de génération de mots fléchés.
#[derive(Debug, Clone)]
pub struct FlechesOptions {
    pub width: usize,
    pub height: usize,
    /// Proportion de cases définitions semées à l'intérieur (hors première
    /// rangée et première colonne).
    pub inner_ratio: f64,
    /// Longueur maximale d'un mot.
    pub max_word_len: usize,
    pub seed: u64,
    /// Nombre de gabarits essayés.
    pub attempts: u32,
    /// Nœuds alloués au remplissage de chaque gabarit.
    pub node_budget: u64,
    /// Voir [`crate::FillOptions::score_weight`].
    pub score_weight: f32,
}

/// Réglages d'un niveau de difficulté.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelSettings {
    /// Seuil de fréquence conseillé avec la liste par défaut
    /// ([`crate::wordlist::french`]).
    pub min_score: u32,
    /// Préférence pour les mots courants (voir [`crate::FillOptions::score_weight`]).
    pub score_weight: f32,
    /// Facteur appliqué au taux de cases définitions intérieures : moins de
    /// cases, des mots plus longs.
    pub density: f64,
}

/// Réglages du niveau `level` (1 : facile, 4 : très difficile).
pub fn level_settings(level: u8) -> LevelSettings {
    let (min_score, score_weight, density) = match level.clamp(1, 4) {
        1 => (22, 1.5, 1.10),
        2 => (17, 0.5, 1.0),
        3 => (12, 0.0, 0.92),
        _ => (8, -0.5, 0.85),
    };
    LevelSettings {
        min_score,
        score_weight,
        density,
    }
}

impl FlechesOptions {
    /// Applique la préférence de vocabulaire et la densité d'un niveau.
    pub fn with_level(mut self, level: u8) -> Self {
        let l = level_settings(level);
        self.score_weight = l.score_weight;
        self.inner_ratio *= l.density;
        self
    }

    pub fn new(width: usize, height: usize) -> Self {
        FlechesOptions {
            width,
            height,
            inner_ratio: default_inner_ratio(width, height),
            max_word_len: width.max(height),
            seed: 0,
            attempts: 1_000,
            node_budget: 200,
            score_weight: 1.0,
        }
    }
}

/// Taux de cases définitions intérieures par défaut, croissant avec la
/// taille : environ 20 % de cases définitions au total en 9×11, 22 % en 13×15.
pub fn default_inner_ratio(width: usize, height: usize) -> f64 {
    let side = ((width * height) as f64).sqrt();
    (0.10 + (side - 8.0) * 0.012).clamp(0.10, 0.18)
}

/// Gabarit valide pour les mots fléchés : mots tous définissables, au plus
/// deux définitions par case, aucune case définition inutile, aucune case
/// lettre hors de tout mot, cases lettres d'un seul tenant.
pub fn is_valid_fleches_pattern(g: &Grid) -> bool {
    g.isolated_cells().is_empty() && g.is_connected() && assign_clues(g, true).is_ok()
}

fn run_len(g: &Grid, r: usize, c: usize, across: bool) -> usize {
    g.slots()
        .into_iter()
        .find(|s| s.across == across && s.cells.contains(&(r * g.width() + c)))
        .map_or(1, |s| s.len())
}

/// Tire un gabarit de mots fléchés.
pub fn random_fleches_pattern(opts: &FlechesOptions, rng: &mut Rng) -> Option<Grid> {
    (0..64).find_map(|_| try_fleches_pattern(opts, rng))
}

fn try_fleches_pattern(opts: &FlechesOptions, rng: &mut Rng) -> Option<Grid> {
    let (w, h) = (opts.width, opts.height);
    let mut g = Grid::new(w, h);
    for c in (0..w).step_by(2) {
        g.set(0, c, Cell::Block);
    }
    for r in (0..h).step_by(2) {
        g.set(r, 0, Cell::Block);
    }
    if !is_valid_fleches_pattern(&g) {
        return None;
    }
    let inner = (w - 1) * (h - 1);
    let target = (inner as f64 * opts.inner_ratio).round() as usize;
    let mut placed = 0;
    let mut rejected = vec![false; w * h];
    loop {
        let slots = g.slots();
        let too_long = slots.iter().any(|s| s.len() > opts.max_word_len);
        if placed >= target && !too_long {
            return Some(g);
        }
        let mut weights = vec![0.0f64; w * h];
        for r in 1..h {
            for c in 1..w {
                let i = r * w + c;
                if rejected[i] || g.get(r, c).is_block() {
                    continue;
                }
                let a = run_len(&g, r, c, true);
                let d = run_len(&g, r, c, false);
                if too_long && a <= opts.max_word_len && d <= opts.max_word_len {
                    continue;
                }
                weights[i] = (a * a + d * d) as f64;
            }
        }
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = rng.unit() * total;
        let mut pick = weights.len() - 1;
        for (i, &wt) in weights.iter().enumerate() {
            if x < wt {
                pick = i;
                break;
            }
            x -= wt;
        }
        let (r, c) = (pick / w, pick % w);
        g.set(r, c, Cell::Block);
        if is_valid_fleches_pattern(&g) {
            placed += 1;
            rejected.iter_mut().for_each(|x| *x = false);
        } else {
            g.set(r, c, Cell::Open(None));
            rejected[pick] = true;
        }
    }
}

/// Génère une grille de mots fléchés.
pub fn generate_fleches(
    dict: &Dictionary,
    opts: &FlechesOptions,
) -> Result<(Fleches, crate::Stats), Error> {
    if opts.width < 3 || opts.height < 3 {
        return Err(Error::InvalidOptions(
            "la grille doit faire au moins 3×3".into(),
        ));
    }
    if !(0.0..0.5).contains(&opts.inner_ratio) {
        return Err(Error::InvalidOptions(
            "le taux de cases définitions doit être dans [0 ; 0,5[".into(),
        ));
    }
    let mut opts = opts.clone();
    opts.max_word_len = opts.max_word_len.min(dict.max_len());
    let mut rng = Rng::new(opts.seed);
    let mut total = 0;
    for attempt in 1..=opts.attempts.max(1) {
        let mut prng = Rng::new(rng.next_u64());
        let Some(grid) = random_fleches_pattern(&opts, &mut prng) else {
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
            let f = Fleches::from_solution(sol.expect("solution présente"), true)
                .map_err(Error::InvalidOptions)?;
            return Ok((
                f,
                crate::Stats {
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

/// Remplit un gabarit de mots fléchés donné (`#` = case définition).
pub fn fill_fleches(
    dict: &Dictionary,
    grid: &Grid,
    opts: &crate::FillOptions,
) -> Result<Fleches, Error> {
    assign_clues(grid, false).map_err(Error::InvalidOptions)?;
    let sol = crate::fill(dict, grid, opts)?;
    Fleches::from_solution(sol, false).map_err(Error::InvalidOptions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_edge_layout_is_valid() {
        for (w, h) in [(5, 5), (6, 7), (9, 12), (12, 9)] {
            let mut g = Grid::new(w, h);
            for c in (0..w).step_by(2) {
                g.set(0, c, Cell::Block);
            }
            for r in (0..h).step_by(2) {
                g.set(r, 0, Cell::Block);
            }
            assert!(is_valid_fleches_pattern(&g), "{w}x{h}\n{g}");
        }
    }

    #[test]
    fn arrows() {
        // # . # . #
        // . . . . .
        // # . . . .
        // . . . . .
        // # . . . .
        let g = Grid::parse("#.#.#\n.....\n#....\n.....\n#....").unwrap();
        let clues = assign_clues(&g, true).unwrap();
        let at = |r, c| {
            let mut v: Vec<Arrow> = clues
                .iter()
                .filter(|k| k.row == r && k.col == c)
                .map(|k| k.arrow)
                .collect();
            v.sort();
            v
        };
        assert_eq!(at(0, 0), vec![Arrow::RightDown, Arrow::DownRight]);
        assert_eq!(at(0, 2), vec![Arrow::RightDown, Arrow::Down]);
        assert_eq!(at(0, 4), vec![Arrow::Down]);
        assert_eq!(at(2, 0), vec![Arrow::Right, Arrow::DownRight]);
        assert_eq!(at(4, 0), vec![Arrow::Right]);
        assert_eq!(clues.len(), 8);
    }

    #[test]
    fn rejects_unclued_word() {
        // Mot horizontal en rangée I partant du bord gauche, rien au-dessus.
        let g = Grid::parse("...\n#..\n...").unwrap();
        assert!(assign_clues(&g, false).is_err());
    }

    #[test]
    fn rejects_more_than_two_clues() {
        // La case du coin devrait porter →, ↳ et ↴.
        let g = Grid::parse("#..\n...\n#..").unwrap();
        assert!(
            assign_clues(&g, false)
                .unwrap_err()
                .contains("plus de deux")
        );
    }

    #[test]
    fn rejects_useless_definition_cell_when_strict() {
        let g = Grid::parse("#.#.\n....\n#.#.").unwrap();
        assert!(assign_clues(&g, false).is_ok());
        assert!(assign_clues(&g, true).is_err());
    }

    #[test]
    fn random_patterns_are_valid() {
        for seed in 0..20 {
            let opts = FlechesOptions::new(9, 11);
            let g = random_fleches_pattern(&opts, &mut Rng::new(seed)).expect("gabarit");
            assert!(is_valid_fleches_pattern(&g));
            assert!(g.slots().iter().all(|s| s.len() <= opts.max_word_len));
        }
    }

    #[test]
    fn definitions_lookup() {
        let txt = "ETE\tSaison chaude\nété;Participe passé\nchat\tFélin\n";
        let d = Definitions::from_reader(txt.as_bytes()).unwrap();
        assert_eq!(d.len(), 2);
        assert!(d.get("Été").is_some());
        assert_eq!(d.get("CHAT"), Some("Félin"));
        assert!(d.get("CHIEN").is_none());
    }

    #[test]
    fn numbering_and_numbered_svg() {
        let g = Grid::parse("#A#B#\nCDEFG\n#HIJK\nLMNOP\n#QRST").unwrap();
        let fl = Fleches::from_solution(Solution::from_grid(g), true).unwrap();
        let nums = fl.numbered_clues();
        assert_eq!(nums.len(), fl.clues.len());
        // Ordre de lecture : la case I 1 (deux définitions) d'abord.
        assert_eq!((nums[0].1.row, nums[0].1.col), (0, 0));
        assert_eq!((nums[1].1.row, nums[1].1.col), (0, 0));
        assert_eq!(nums[0].1.arrow, Arrow::RightDown);
        assert!(nums.iter().enumerate().all(|(i, (n, _))| *n == i + 1));
        let mut defs = Definitions::default();
        defs.insert("CDEFG", "Une & définition");
        let svg = fl.to_svg_numbered(false, Some(&defs));
        assert!(svg.contains(">Définitions<"));
        assert!(svg.contains("Une &amp; définition"));
        assert!(svg.contains(&format!(">{}<", nums.len())));
        assert!(!fl.to_svg(false, Some(&defs)).contains(">Définitions<"));
        assert!(
            fl.to_svg_numbered_in(false, Some(&defs), Language::En)
                .contains(">Clues<")
        );
        let list = fl.numbered_clue_list(Some(&defs));
        assert_eq!(list.lines().count(), nums.len());
    }

    #[test]
    fn definitions_by_level() {
        let txt = "ETE\tSaison chaude\t1\nETE\tParticipe au passé\t4\nOR\tMétal\nSEL\tCondiment\t1\nMER\tVague ; écume\t9\n";
        let d = Definitions::from_reader(txt.as_bytes()).unwrap();
        assert_eq!(d.at_level(1).get("ETE"), Some("Saison chaude"));
        assert_eq!(d.at_level(4).get("ETE"), Some("Participe au passé"));
        // Sans niveau : valable partout.
        assert_eq!(d.at_level(3).get("OR"), Some("Métal"));
        // Niveau 1 seulement : absent au niveau 3.
        assert!(d.at_level(3).get("SEL").is_none());
        // Dernier champ hors de 1..=4 : il fait partie de la définition.
        assert_eq!(d.at_level(2).get("MER"), Some("Vague ; écume\t9"));
    }

    #[test]
    fn levels_get_harder() {
        let l: Vec<LevelSettings> = LEVELS.map(level_settings).collect();
        for w in l.windows(2) {
            assert!(w[1].min_score < w[0].min_score);
            assert!(w[1].score_weight < w[0].score_weight);
            assert!(w[1].density < w[0].density);
        }
        let easy = FlechesOptions::new(11, 13).with_level(1);
        let hard = FlechesOptions::new(11, 13).with_level(4);
        assert!(hard.inner_ratio < easy.inner_ratio);
    }

    #[test]
    fn user_definitions_take_precedence() {
        let mut user = Definitions::from_reader("NI\tA moi\n".as_bytes()).unwrap();
        let cache = Definitions::from_reader("NI\tCache\nOR\tMétal\n".as_bytes()).unwrap();
        user.extend_missing(cache);
        assert_eq!(user.get("NI"), Some("A moi"));
        assert_eq!(user.get("OR"), Some("Métal"));
    }

    #[test]
    fn wrap_text() {
        assert_eq!(
            wrap("Saison la plus chaude", 11, 3),
            vec!["Saison la", "plus chaude"]
        );
        assert_eq!(wrap("a b c d e f g h", 1, 2), vec!["a", "b…"]);
    }
}
