//! Grille, emplacements de mots et rendus (texte, liste de mots, SVG).

use crate::dict::{Letter, letter_char, normalize};
use std::fmt::{self, Write as _};

/// Contenu d'une case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Case noire.
    Block,
    /// Case blanche, avec ou sans lettre.
    Open(Option<Letter>),
}

impl Cell {
    pub fn is_block(self) -> bool {
        matches!(self, Cell::Block)
    }
}

/// Une grille rectangulaire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

/// Erreur de lecture d'un gabarit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gabarit invalide : {}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl Grid {
    /// Grille entièrement blanche et vide.
    pub fn new(width: usize, height: usize) -> Self {
        Grid {
            width,
            height,
            cells: vec![Cell::Open(None); width * height],
        }
    }

    /// Lit un gabarit texte, une ligne par rangée :
    /// `#` ou `*` = case noire, `.` ou `_` = case vide, une lettre = lettre imposée.
    /// Les lignes vides en début et fin sont ignorées, les accents sont retirés.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let rows: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .skip_while(|l| l.trim().is_empty())
            .collect();
        let rows: Vec<&str> = {
            let end = rows
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map_or(0, |i| i + 1);
            rows[..end].to_vec()
        };
        if rows.is_empty() {
            return Err(ParseError("gabarit vide".into()));
        }
        let mut cells = Vec::new();
        let mut width = None;
        for (r, row) in rows.iter().enumerate() {
            let mut n = 0;
            for ch in row.chars() {
                let cell = match ch {
                    '#' | '*' | '■' => Cell::Block,
                    '.' | '_' => Cell::Open(None),
                    c => match normalize(&c.to_string()).as_deref() {
                        Some([l]) => Cell::Open(Some(*l)),
                        _ => {
                            return Err(ParseError(format!(
                                "caractère {c:?} inattendu en rangée {}",
                                r + 1
                            )));
                        }
                    },
                };
                cells.push(cell);
                n += 1;
            }
            match width {
                None => width = Some(n),
                Some(w) if w != n => {
                    return Err(ParseError(format!(
                        "la rangée {} fait {n} cases au lieu de {w}",
                        r + 1
                    )));
                }
                _ => {}
            }
        }
        let width = width.unwrap_or(0);
        if width == 0 {
            return Err(ParseError("rangées vides".into()));
        }
        Ok(Grid {
            width,
            height: rows.len(),
            cells,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, row: usize, col: usize) -> Cell {
        self.cells[row * self.width + col]
    }

    pub fn set(&mut self, row: usize, col: usize, cell: Cell) {
        self.cells[row * self.width + col] = cell;
    }

    pub(crate) fn cell(&self, i: usize) -> Cell {
        self.cells[i]
    }

    pub(crate) fn set_cell(&mut self, i: usize, cell: Cell) {
        self.cells[i] = cell;
    }

    pub(crate) fn len(&self) -> usize {
        self.cells.len()
    }

    /// Nombre de cases noires.
    pub fn block_count(&self) -> usize {
        self.cells.iter().filter(|c| c.is_block()).count()
    }

    /// Emplacements de mots (suites maximales d'au moins deux cases blanches),
    /// les horizontaux d'abord, en ordre de lecture.
    pub fn slots(&self) -> Vec<Slot> {
        let mut out = Vec::new();
        for across in [true, false] {
            let (outer, inner) = if across {
                (self.height, self.width)
            } else {
                (self.width, self.height)
            };
            for a in 0..outer {
                let mut b = 0;
                while b < inner {
                    let idx = |b: usize| {
                        if across {
                            a * self.width + b
                        } else {
                            b * self.width + a
                        }
                    };
                    if self.cells[idx(b)].is_block() {
                        b += 1;
                        continue;
                    }
                    let start = b;
                    while b < inner && !self.cells[idx(b)].is_block() {
                        b += 1;
                    }
                    if b - start >= 2 {
                        let (row, col) = if across { (a, start) } else { (start, a) };
                        out.push(Slot {
                            across,
                            row,
                            col,
                            cells: (start..b).map(idx).collect(),
                        });
                    }
                }
            }
        }
        out
    }

    /// Cases blanches qui n'appartiennent à aucun mot (encadrées dans les deux sens).
    pub fn isolated_cells(&self) -> Vec<(usize, usize)> {
        let mut covered = vec![false; self.cells.len()];
        for s in self.slots() {
            for &c in &s.cells {
                covered[c] = true;
            }
        }
        (0..self.cells.len())
            .filter(|&i| !self.cells[i].is_block() && !covered[i])
            .map(|i| (i / self.width, i % self.width))
            .collect()
    }

    /// Vrai si les cases blanches forment une seule composante (4-connexité).
    pub fn is_connected(&self) -> bool {
        let Some(start) = self.cells.iter().position(|c| !c.is_block()) else {
            return true;
        };
        let mut seen = vec![false; self.cells.len()];
        let mut stack = vec![start];
        seen[start] = true;
        let mut n = 0;
        while let Some(i) = stack.pop() {
            n += 1;
            for j in self.neighbors(i) {
                if !seen[j] && !self.cells[j].is_block() {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        n == self.cells.iter().filter(|c| !c.is_block()).count()
    }

    pub(crate) fn neighbors(&self, i: usize) -> impl Iterator<Item = usize> + use<> {
        let (w, h) = (self.width, self.height);
        let (r, c) = (i / w, i % w);
        [
            (r > 0).then(|| i - w),
            (r + 1 < h).then(|| i + w),
            (c > 0).then(|| i - 1),
            (c + 1 < w).then(|| i + 1),
        ]
        .into_iter()
        .flatten()
    }

    /// Texte du mot (ou du motif, `.` pour les cases vides) d'un emplacement.
    pub fn slot_text(&self, slot: &Slot) -> String {
        slot.cells
            .iter()
            .map(|&i| match self.cells[i] {
                Cell::Open(Some(l)) => letter_char(l),
                _ => '.',
            })
            .collect()
    }

    /// Rendu SVG imprimable. Avec `with_letters = false`, on obtient la grille
    /// vierge à remplir ; avec `true`, la solution.
    pub fn to_svg(&self, with_letters: bool) -> String {
        const C: usize = 40; // côté d'une case
        const M: usize = 36; // marge pour la numérotation
        let w = M + self.width * C + 4;
        let h = M + self.height * C + 4;
        let mut s = String::new();
        let _ = write!(
            s,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" font-family="Georgia, serif">"#
        );
        s.push_str(r#"<rect width="100%" height="100%" fill="white"/>"#);
        for c in 0..self.width {
            let _ = write!(
                s,
                r#"<text x="{}" y="{}" font-size="16" text-anchor="middle">{}</text>"#,
                M + c * C + C / 2,
                M - 10,
                c + 1
            );
        }
        for r in 0..self.height {
            let _ = write!(
                s,
                r#"<text x="{}" y="{}" font-size="14" text-anchor="end">{}</text>"#,
                M - 8,
                M + r * C + C / 2 + 5,
                roman(r + 1)
            );
        }
        for r in 0..self.height {
            for c in 0..self.width {
                let (x, y) = (M + c * C, M + r * C);
                match self.get(r, c) {
                    Cell::Block => {
                        let _ = write!(
                            s,
                            r#"<rect x="{x}" y="{y}" width="{C}" height="{C}" fill="black"/>"#
                        );
                    }
                    Cell::Open(l) => {
                        let _ = write!(
                            s,
                            r#"<rect x="{x}" y="{y}" width="{C}" height="{C}" fill="white" stroke="black" stroke-width="1"/>"#
                        );
                        if let (true, Some(l)) = (with_letters, l) {
                            let _ = write!(
                                s,
                                r#"<text x="{}" y="{}" font-size="24" text-anchor="middle">{}</text>"#,
                                x + C / 2,
                                y + C / 2 + 8,
                                letter_char(l)
                            );
                        }
                    }
                }
            }
        }
        let _ = write!(
            s,
            r#"<rect x="{M}" y="{M}" width="{}" height="{}" fill="none" stroke="black" stroke-width="3"/>"#,
            self.width * C,
            self.height * C
        );
        s.push_str("</svg>\n");
        s
    }
}

impl fmt::Display for Grid {
    /// Rendu texte : `#` pour les noires, `.` pour les cases vides.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in 0..self.height {
            for c in 0..self.width {
                let ch = match self.get(r, c) {
                    Cell::Block => '#',
                    Cell::Open(None) => '.',
                    Cell::Open(Some(l)) => letter_char(l),
                };
                f.write_char(ch)?;
            }
            f.write_char('\n')?;
        }
        Ok(())
    }
}

/// Emplacement d'un mot dans la grille.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// `true` pour un mot horizontal.
    pub across: bool,
    /// Rangée de la première lettre (0 = rangée I).
    pub row: usize,
    /// Colonne de la première lettre (0 = colonne 1).
    pub col: usize,
    /// Index des cases (`row * width + col`).
    pub cells: Vec<usize>,
}

impl Slot {
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// Mot placé dans une grille remplie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub across: bool,
    pub row: usize,
    pub col: usize,
    pub word: String,
}

/// Grille remplie, avec la liste de ses mots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Solution {
    pub grid: Grid,
    pub entries: Vec<Entry>,
}

impl Solution {
    pub(crate) fn from_grid(grid: Grid) -> Self {
        let entries = grid
            .slots()
            .iter()
            .map(|s| Entry {
                across: s.across,
                row: s.row,
                col: s.col,
                word: grid.slot_text(s),
            })
            .collect();
        Solution { grid, entries }
    }

    pub fn across(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.across)
    }

    pub fn down(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !e.across)
    }

    /// Liste des mots à la française : « Horizontalement » par rangée en
    /// chiffres romains, « Verticalement » par colonne en chiffres arabes,
    /// les mots d'une même ligne séparés par des tirets.
    pub fn word_list(&self) -> String {
        let mut s = String::from("Horizontalement\n");
        for r in 0..self.grid.height() {
            let words: Vec<&str> = self
                .across()
                .filter(|e| e.row == r)
                .map(|e| e.word.as_str())
                .collect();
            if !words.is_empty() {
                let _ = writeln!(s, "  {:>5}. {}", roman(r + 1), words.join(" - "));
            }
        }
        s.push_str("Verticalement\n");
        for c in 0..self.grid.width() {
            let words: Vec<&str> = self
                .down()
                .filter(|e| e.col == c)
                .map(|e| e.word.as_str())
                .collect();
            if !words.is_empty() {
                let _ = writeln!(s, "  {:>5}. {}", c + 1, words.join(" - "));
            }
        }
        s
    }
}

impl fmt::Display for Solution {
    /// Grille avec en-têtes de colonnes et de rangées.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let g = &self.grid;
        // Colonnes de 3 caractères dès que les numéros ont deux chiffres.
        let cw = if g.width() >= 10 { 3 } else { 2 };
        write!(f, "      ")?;
        for c in 0..g.width() {
            write!(f, "{:>cw$}", c + 1)?;
        }
        writeln!(f)?;
        for r in 0..g.height() {
            write!(f, "{:>5} ", roman(r + 1))?;
            for c in 0..g.width() {
                let ch = match g.get(r, c) {
                    Cell::Block => '■',
                    Cell::Open(None) => '.',
                    Cell::Open(Some(l)) => letter_char(l),
                };
                write!(f, "{ch:>cw$}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Chiffres romains (1 à 3999).
pub fn roman(mut n: usize) -> String {
    const T: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for &(v, r) in &T {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_numerals() {
        assert_eq!(roman(1), "I");
        assert_eq!(roman(4), "IV");
        assert_eq!(roman(9), "IX");
        assert_eq!(roman(14), "XIV");
        assert_eq!(roman(1994), "MCMXCIV");
    }

    #[test]
    fn parse_and_slots() {
        let g = Grid::parse("\nAB#.\n..#.\n#...\n").unwrap();
        assert_eq!((g.width(), g.height()), (4, 3));
        assert_eq!(g.block_count(), 3);
        let slots = g.slots();
        let across: Vec<_> = slots
            .iter()
            .filter(|s| s.across)
            .map(|s| (s.row, s.col, s.len()))
            .collect();
        let down: Vec<_> = slots
            .iter()
            .filter(|s| !s.across)
            .map(|s| (s.row, s.col, s.len()))
            .collect();
        assert_eq!(across, vec![(0, 0, 2), (1, 0, 2), (2, 1, 3)]);
        assert_eq!(down, vec![(0, 0, 2), (0, 1, 3), (0, 3, 3)]);
        assert_eq!(g.slot_text(&slots[0]), "AB");
    }

    #[test]
    fn parse_errors() {
        assert!(Grid::parse("").is_err());
        assert!(Grid::parse("...\n..").is_err());
        assert!(Grid::parse("..1").is_err());
    }

    #[test]
    fn isolated_and_connectivity() {
        let g = Grid::parse(".#.\n#..\n...").unwrap();
        assert_eq!(g.isolated_cells(), vec![(0, 0)]);
        assert!(!g.is_connected());
        let g = Grid::parse("..#\n...\n#..").unwrap();
        assert!(g.isolated_cells().is_empty());
        assert!(g.is_connected());
    }
}
