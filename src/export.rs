//! Export JSON des grilles, documenté dans `docs/format-json.md`.

use crate::fleches::{Arrow, Definitions, Fleches};
use crate::grid::{Cell, Solution};
use crate::json::quote;
use std::fmt::Write as _;

/// Version du format, à incrémenter à tout changement incompatible.
pub const JSON_FORMAT_VERSION: u32 = 1;

impl Arrow {
    /// Nom de la flèche dans l'export JSON.
    pub fn json_name(self) -> &'static str {
        match self {
            Arrow::Right => "droite",
            Arrow::Down => "bas",
            Arrow::RightDown => "droite-bas",
            Arrow::DownRight => "bas-droite",
        }
    }
}

fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or("null".into(), |v| v.to_string())
}

/// Document complet : en-tête, grille, mots, puis `definitions` déjà
/// sérialisées (une entrée par ligne).
fn document(
    kind: &str,
    sol: &Solution,
    seed: Option<u64>,
    level: Option<u8>,
    definitions: &[String],
) -> String {
    let g = &sol.grid;
    let mut s = String::from("{\n");
    let _ = writeln!(s, "  \"format\": \"crosswords\",");
    let _ = writeln!(s, "  \"version\": {JSON_FORMAT_VERSION},");
    let _ = writeln!(s, "  \"type\": {},", quote(kind));
    let _ = writeln!(s, "  \"largeur\": {},", g.width());
    let _ = writeln!(s, "  \"hauteur\": {},", g.height());
    let _ = writeln!(s, "  \"graine\": {},", opt(seed));
    let _ = writeln!(s, "  \"niveau\": {},", opt(level));
    s.push_str("  \"grille\": [\n");
    for r in 0..g.height() {
        let row: String = (0..g.width())
            .map(|c| match g.get(r, c) {
                Cell::Block => '#',
                Cell::Open(None) => '.',
                Cell::Open(Some(l)) => crate::letter_char(l),
            })
            .collect();
        let sep = if r + 1 < g.height() { "," } else { "" };
        let _ = writeln!(s, "    {}{sep}", quote(&row));
    }
    s.push_str("  ],\n  \"mots\": [\n");
    for (i, e) in sol.entries.iter().enumerate() {
        let sep = if i + 1 < sol.entries.len() { "," } else { "" };
        let _ = writeln!(
            s,
            "    {{\"id\": {i}, \"mot\": {}, \"sens\": \"{}\", \"ligne\": {}, \"colonne\": {}, \"longueur\": {}}}{sep}",
            quote(&e.word),
            if e.across { "horizontal" } else { "vertical" },
            e.row,
            e.col,
            e.word.len()
        );
    }
    s.push_str("  ],\n  \"definitions\": [");
    if definitions.is_empty() {
        s.push_str("]\n}\n");
        return s;
    }
    s.push('\n');
    for (i, d) in definitions.iter().enumerate() {
        let sep = if i + 1 < definitions.len() { "," } else { "" };
        let _ = writeln!(s, "    {d}{sep}");
    }
    s.push_str("  ]\n}\n");
    s
}

impl Solution {
    /// Export JSON d'une grille de mots croisés (format décrit dans
    /// `docs/format-json.md`).
    pub fn to_json(&self, seed: Option<u64>) -> String {
        document("croises", self, seed, None, &[])
    }
}

impl Fleches {
    /// Export JSON d'une grille de mots fléchés : définitions numérotées en
    /// ordre de lecture, comme dans le rendu à numéros.
    pub fn to_json(
        &self,
        defs: Option<&Definitions>,
        seed: Option<u64>,
        level: Option<u8>,
    ) -> String {
        let definitions: Vec<String> = self
            .numbered_clues()
            .into_iter()
            .map(|(n, c)| {
                let word = &self.solution.entries[c.entry].word;
                format!(
                    "{{\"numero\": {n}, \"ligne\": {}, \"colonne\": {}, \"fleche\": \"{}\", \"mot\": {}, \"texte\": {}}}",
                    c.row,
                    c.col,
                    c.arrow.json_name(),
                    c.entry,
                    defs.and_then(|d| d.get(word)).map_or("null".into(), quote)
                )
            })
            .collect();
        document("fleches", &self.solution, seed, level, &definitions)
    }
}

#[cfg(test)]
mod tests {
    use crate::json::{Json, parse};
    use crate::{Definitions, Dictionary, FillOptions, Fleches, Grid, Solution, fill};

    fn num(v: &Json, k: &str) -> usize {
        v.get(k).and_then(Json::as_f64).unwrap() as usize
    }

    #[test]
    fn crossword_json() {
        let dict = Dictionary::from_words(["ABC", "DEF", "GHI", "ADG", "BEH", "CFI"]);
        let sol = fill(
            &dict,
            &Grid::parse("...\n...\n...").unwrap(),
            &FillOptions::default(),
        )
        .unwrap();
        let v = parse(&sol.to_json(Some(7))).expect("JSON valide");
        assert_eq!(v.get("type").and_then(Json::as_str), Some("croises"));
        assert_eq!(num(&v, "graine"), 7);
        assert_eq!(v.get("niveau"), Some(&Json::Null));
        assert_eq!(
            v.get("definitions").and_then(Json::as_arr).unwrap().len(),
            0
        );
        let rows: Vec<&str> = v
            .get("grille")
            .and_then(Json::as_arr)
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect();
        let mots = v.get("mots").and_then(Json::as_arr).unwrap();
        assert_eq!(mots.len(), 6);
        // Chaque mot se relit dans la grille à sa position.
        for m in mots {
            let (r, c, n) = (num(m, "ligne"), num(m, "colonne"), num(m, "longueur"));
            let across = m.get("sens").and_then(Json::as_str) == Some("horizontal");
            let read: String = (0..n)
                .map(|k| {
                    let (rr, cc) = if across { (r, c + k) } else { (r + k, c) };
                    rows[rr].as_bytes()[cc] as char
                })
                .collect();
            assert_eq!(Some(read.as_str()), m.get("mot").and_then(Json::as_str));
        }
    }

    #[test]
    fn fleches_json() {
        let g = Grid::parse("#A#B#\nCDEFG\n#HIJK\nLMNOP\n#QRST").unwrap();
        let fl = Fleches::from_solution(Solution::from_grid(g), true).unwrap();
        let mut defs = Definitions::default();
        defs.insert("CDEFG", "Une \"définition\"");
        let v = parse(&fl.to_json(Some(&defs), Some(3), Some(2))).expect("JSON valide");
        assert_eq!(v.get("type").and_then(Json::as_str), Some("fleches"));
        assert_eq!(num(&v, "niveau"), 2);
        let ds = v.get("definitions").and_then(Json::as_arr).unwrap();
        assert_eq!(ds.len(), fl.clues.len());
        for (i, d) in ds.iter().enumerate() {
            assert_eq!(num(d, "numero"), i + 1);
            let e = num(d, "mot");
            let texte = d.get("texte").unwrap();
            if fl.solution.entries[e].word == "CDEFG" {
                assert_eq!(texte.as_str(), Some("Une \"définition\""));
                assert_eq!(d.get("fleche").and_then(Json::as_str), Some("bas-droite"));
                assert_eq!((num(d, "ligne"), num(d, "colonne")), (0, 0));
            } else {
                assert_eq!(texte, &Json::Null);
            }
        }
    }
}
