//! Dictionnaire indexé pour le remplissage.
//!
//! Les mots sont normalisés en majuscules sans accents (`été` → `ETE`,
//! `cœur` → `COEUR`), puis rangés par longueur. Pour chaque longueur `L`,
//! on construit `L × 26` bitsets : le bitset `(p, c)` contient les mots dont la
//! lettre en position `p` est `c`. Le nombre de mots compatibles avec un motif
//! (`A.B..`) s'obtient alors par un ET des bitsets des positions fixées, suivi
//! d'un popcount, sans parcourir les mots.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead};

/// Une lettre normalisée, `0..26` pour `A..Z`.
pub type Letter = u8;

/// Mots d'une longueur donnée et leurs index.
#[derive(Debug, Default)]
pub(crate) struct Bucket {
    pub(crate) len: usize,
    /// Lettres à plat : le mot `i` occupe `letters[i*len .. (i+1)*len]`.
    pub(crate) letters: Vec<Letter>,
    pub(crate) scores: Vec<u32>,
    /// Nombre de mots de 64 bits par bitset.
    pub(crate) nw: usize,
    /// `index[((pos * 26) + letter) * nw ..][..nw]`.
    index: Vec<u64>,
}

impl Bucket {
    pub(crate) fn count(&self) -> usize {
        self.scores.len()
    }

    pub(crate) fn word(&self, i: usize) -> &[Letter] {
        &self.letters[i * self.len..(i + 1) * self.len]
    }

    /// Bitset des mots ayant la lettre `letter` en position `pos`.
    #[inline]
    pub(crate) fn slice(&self, pos: usize, letter: Letter) -> &[u64] {
        let off = (pos * 26 + letter as usize) * self.nw;
        &self.index[off..off + self.nw]
    }

    /// Masque du dernier mot de 64 bits (les bits au-delà de `count()` sont faux).
    pub(crate) fn tail_mask(&self) -> u64 {
        match self.count() % 64 {
            0 => u64::MAX,
            r => (1u64 << r) - 1,
        }
    }

    /// Itère, mot de 64 bits par mot de 64 bits, sur l'ensemble des mots qui
    /// respectent `pattern` et ne sont pas dans `used`.
    fn for_each_block(
        &self,
        pattern: &[Option<Letter>],
        used: &[u64],
        mut f: impl FnMut(usize, u64),
    ) {
        debug_assert_eq!(pattern.len(), self.len);
        let fixed: Vec<&[u64]> = pattern
            .iter()
            .enumerate()
            .filter_map(|(p, l)| l.map(|l| self.slice(p, l)))
            .collect();
        let tail = self.tail_mask();
        for w in 0..self.nw {
            let mut acc = !used[w];
            if w + 1 == self.nw {
                acc &= tail;
            }
            for s in &fixed {
                acc &= s[w];
                if acc == 0 {
                    break;
                }
            }
            if acc != 0 {
                f(w, acc);
            }
        }
    }

    pub(crate) fn count_matching(&self, pattern: &[Option<Letter>], used: &[u64]) -> u32 {
        let mut n = 0;
        self.for_each_block(pattern, used, |_, acc| n += acc.count_ones());
        n
    }

    #[cfg(test)]
    pub(crate) fn matching(&self, pattern: &[Option<Letter>], used: &[u64]) -> Vec<u32> {
        let mut out = Vec::new();
        self.for_each_block(pattern, used, |w, mut acc| {
            while acc != 0 {
                let b = acc.trailing_zeros() as usize;
                out.push((w * 64 + b) as u32);
                acc &= acc - 1;
            }
        });
        out
    }
}

/// Dictionnaire prêt pour le remplissage de grilles.
#[derive(Debug, Default)]
pub struct Dictionary {
    pub(crate) buckets: Vec<Bucket>,
}

impl Dictionary {
    /// Construit un dictionnaire à partir de couples `(mot, score)`.
    ///
    /// Les mots sont normalisés ; ceux qui contiennent autre chose que des
    /// lettres (tiret, apostrophe, espace, chiffre) ou qui font moins de deux
    /// lettres sont ignorés. Les doublons après normalisation (`ÉTÉ`/`ETE`) sont
    /// fusionnés en gardant le meilleur score.
    pub fn from_scored<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = (S, u32)>,
        S: AsRef<str>,
    {
        let mut by_len: HashMap<usize, HashMap<Vec<Letter>, u32>> = HashMap::new();
        for (w, score) in words {
            if let Some(n) = normalize(w.as_ref()) {
                if n.len() < 2 {
                    continue;
                }
                let e = by_len.entry(n.len()).or_default().entry(n).or_insert(score);
                *e = (*e).max(score);
            }
        }
        let max_len = by_len.keys().copied().max().unwrap_or(0);
        let mut buckets: Vec<Bucket> = (0..=max_len)
            .map(|len| Bucket {
                len,
                ..Default::default()
            })
            .collect();
        for (len, words) in by_len {
            // Tri pour un ordre stable, donc des grilles reproductibles.
            let mut words: Vec<_> = words.into_iter().collect();
            words.sort_unstable();
            let n = words.len();
            let nw = n.div_ceil(64);
            let mut b = Bucket {
                len,
                letters: Vec::with_capacity(n * len),
                scores: Vec::with_capacity(n),
                nw,
                index: vec![0; len * 26 * nw],
            };
            for (i, (w, s)) in words.into_iter().enumerate() {
                for (p, &l) in w.iter().enumerate() {
                    b.index[(p * 26 + l as usize) * nw + i / 64] |= 1 << (i % 64);
                }
                b.letters.extend_from_slice(&w);
                b.scores.push(s);
            }
            buckets[len] = b;
        }
        Dictionary { buckets }
    }

    /// Construit un dictionnaire où tous les mots ont le même score.
    pub fn from_words<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::from_scored(words.into_iter().map(|w| (w, 1)))
    }

    /// Lit un fichier texte : un mot par ligne, éventuellement suivi d'un score
    /// entier (séparé par tabulation, espace, `;` ou `,`). Les lignes vides et
    /// celles qui commencent par `#` sont ignorées. Les mots dont le score est
    /// strictement inférieur à `min_score` sont écartés.
    pub fn from_reader<R: BufRead>(reader: R, min_score: u32) -> io::Result<Self> {
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Le score éventuel est le dernier champ ; sans score numérique,
            // toute la ligne est le mot (une locution sera donc écartée).
            let (word, score) = match line.rfind(['\t', ' ', ';', ',']) {
                Some(i) => match line[i + 1..].parse::<f64>() {
                    Ok(f) => (line[..i].trim(), f.max(0.0).round() as u32),
                    Err(_) => (line, 1),
                },
                None => (line, 1),
            };
            if score >= min_score {
                entries.push((word.to_string(), score));
            }
        }
        Ok(Self::from_scored(entries))
    }

    pub(crate) fn bucket(&self, len: usize) -> Option<&Bucket> {
        self.buckets.get(len).filter(|b| b.count() > 0)
    }

    /// Nombre total de mots distincts.
    pub fn len(&self) -> usize {
        self.buckets.iter().map(Bucket::count).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Nombre de mots de longueur `len`.
    pub fn count_of_len(&self, len: usize) -> usize {
        self.buckets.get(len).map_or(0, Bucket::count)
    }

    /// Longueur du plus long mot.
    pub fn max_len(&self) -> usize {
        self.buckets
            .iter()
            .rev()
            .find(|b| b.count() > 0)
            .map_or(0, |b| b.len)
    }

    /// Vrai si le mot (après normalisation) est dans le dictionnaire.
    pub fn contains(&self, word: &str) -> bool {
        let Some(n) = normalize(word) else {
            return false;
        };
        let Some(b) = self.bucket(n.len()) else {
            return false;
        };
        let pattern: Vec<_> = n.iter().map(|&l| Some(l)).collect();
        let used = vec![0u64; b.nw];
        b.count_matching(&pattern, &used) > 0
    }
}

impl fmt::Display for Dictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} mots", self.len())
    }
}

/// Normalise un mot : majuscules, accents et ligatures retirés.
/// Renvoie `None` si le mot contient un caractère qui n'est pas une lettre.
pub fn normalize(word: &str) -> Option<Vec<Letter>> {
    let mut out = Vec::with_capacity(word.len());
    for ch in word.chars() {
        let base: &str = match ch {
            'a'..='z' | 'A'..='Z' => {
                out.push(ch.to_ascii_uppercase() as u8 - b'A');
                continue;
            }
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' | 'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => {
                "A"
            }
            'ç' | 'Ç' => "C",
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => "E",
            'î' | 'ï' | 'í' | 'ì' | 'Î' | 'Ï' | 'Í' | 'Ì' => "I",
            'ñ' | 'Ñ' => "N",
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' | 'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => "O",
            'ù' | 'û' | 'ü' | 'ú' | 'Ù' | 'Û' | 'Ü' | 'Ú' => "U",
            'ÿ' | 'Ÿ' | 'ý' | 'Ý' => "Y",
            'œ' | 'Œ' => "OE",
            'æ' | 'Æ' => "AE",
            _ => return None,
        };
        out.extend(base.bytes().map(|b| b - b'A'));
    }
    Some(out)
}

/// Convertit une lettre normalisée en caractère.
#[inline]
pub fn letter_char(l: Letter) -> char {
    (b'A' + l) as char
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[Letter]) -> String {
        v.iter().map(|&l| letter_char(l)).collect()
    }

    #[test]
    fn normalization() {
        assert_eq!(s(&normalize("été").unwrap()), "ETE");
        assert_eq!(s(&normalize("Cœur").unwrap()), "COEUR");
        assert_eq!(s(&normalize("garçon").unwrap()), "GARCON");
        assert!(normalize("porte-monnaie").is_none());
        assert!(normalize("aujourd'hui").is_none());
    }

    #[test]
    fn dedup_and_index() {
        let d = Dictionary::from_scored([("été", 3), ("ete", 7), ("eta", 1), ("x", 1), ("abc", 2)]);
        assert_eq!(d.len(), 3);
        assert_eq!(d.count_of_len(3), 3);
        let b = d.bucket(3).unwrap();
        let used = vec![0; b.nw];
        // Motif E.E
        let p = [Some(4), None, Some(4)];
        let m = b.matching(&p, &used);
        assert_eq!(m.len(), 1);
        assert_eq!(s(b.word(m[0] as usize)), "ETE");
        assert_eq!(b.scores[m[0] as usize], 7);
        assert!(d.contains("Été"));
        assert!(!d.contains("ETC"));
    }

    #[test]
    fn used_words_are_excluded() {
        let d = Dictionary::from_words(["AB", "AC", "AD"]);
        let b = d.bucket(2).unwrap();
        let mut used = vec![0; b.nw];
        assert_eq!(b.count_matching(&[Some(0), None], &used), 3);
        used[0] |= 1 << 1;
        assert_eq!(b.count_matching(&[Some(0), None], &used), 2);
    }

    #[test]
    fn reader_with_scores() {
        let txt = "# commentaire\nchat\t10\nchien 3\n\nloup;20\nsri lankais\t9\na priori\nmer\n";
        let d = Dictionary::from_reader(txt.as_bytes(), 1).unwrap();
        assert_eq!(d.len(), 4);
        assert!(d.contains("chat"));
        assert!(d.contains("chien"));
        assert!(d.contains("mer"));
        assert!(!d.contains("sri"));
        assert!(!d.contains("a"));
        let d = Dictionary::from_reader(txt.as_bytes(), 5).unwrap();
        assert_eq!(d.len(), 2);
        assert!(!d.contains("chien"));
    }

    #[test]
    fn large_bucket_tail_mask() {
        // 70 mots de 2 lettres : vérifie que les bits fantômes du dernier mot
        // de 64 bits ne sont jamais comptés.
        let words: Vec<String> = (0..70)
            .map(|i| {
                format!(
                    "{}{}",
                    (b'A' + (i / 26) as u8) as char,
                    (b'A' + (i % 26) as u8) as char
                )
            })
            .collect();
        let d = Dictionary::from_words(&words);
        let b = d.bucket(2).unwrap();
        let used = vec![0; b.nw];
        assert_eq!(b.count_matching(&[None, None], &used), 70);
        assert_eq!(b.matching(&[None, None], &used).len(), 70);
    }
}
