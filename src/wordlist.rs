//! Liste de mots française téléchargée à la demande et mise en cache.
//!
//! Deux sources, figées par version et vérifiées par empreinte SHA-256 :
//!
//! - [an-array-of-french-words] 2.0.0 (MIT) : 336 000 formes, flexions
//!   comprises ;
//! - [Lexique] 3.83 (CC BY-SA 4.0) : fréquences d'usage (films et livres).
//!
//! Elles sont fusionnées en un fichier `mot<TAB>score` : la liste de formes
//! fixe les mots admis, Lexique ne fait que les noter (ses mots absents de la
//! liste sont surtout des scories de sous-titres : « dano », « ok », « to »).
//! Le score croît avec le logarithme de la fréquence (0 pour un mot absent
//! de Lexique, 21 pour un mot vu une fois par million, 41 pour cent fois par
//! million). Le
//! téléchargement passe par la commande `curl`, présente sur Linux, macOS et
//! Windows 10 et plus.
//!
//! [an-array-of-french-words]: https://github.com/words/an-array-of-french-words
//! [Lexique]: http://www.lexique.org

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Source {
    url: &'static str,
    sha256: &'static str,
}

const WORDS: Source = Source {
    url: "https://raw.githubusercontent.com/words/an-array-of-french-words/2.0.0/index.json",
    sha256: "75e81515ed0714c8d98d6aa0cc71ba3094834653de005f9bdbb874257639e92d",
};

const LEXIQUE: Source = Source {
    url: "https://www.lexique.org/databases/Lexique383/Lexique383.tsv",
    sha256: "637ba37a767a66679c48371d673ece50cbf541b49a4e40e598963d4f3fbce52b",
};

/// Nom du fichier fusionné ; à changer si le format ou le calcul des scores
/// évolue, pour forcer la régénération.
const MERGED: &str = "fr-v2.txt";

/// Dossier de cache : `$MOTSCROISES_CACHE`, sinon `$XDG_CACHE_HOME/motscroises`,
/// sinon `~/.cache/motscroises` (`%LOCALAPPDATA%\motscroises` sous Windows).
pub fn cache_dir() -> Option<PathBuf> {
    let env = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("MOTSCROISES_CACHE")
        .or_else(|| env("XDG_CACHE_HOME").map(|p| p.join("motscroises")))
        .or_else(|| env("LOCALAPPDATA").map(|p| p.join("motscroises")))
        .or_else(|| env("HOME").map(|p| p.join(".cache").join("motscroises")))
}

/// Chemin de la liste française prête à l'emploi, téléchargée et construite
/// au premier appel. `log` reçoit les messages de progression.
pub fn french(mut log: impl FnMut(&str)) -> io::Result<PathBuf> {
    let dir = cache_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "aucun dossier de cache (définir MOTSCROISES_CACHE)",
        )
    })?;
    let merged = dir.join(MERGED);
    if merged.is_file() {
        return Ok(merged);
    }
    fs::create_dir_all(&dir)?;
    log("Première utilisation : téléchargement de la liste de mots (une seule fois)");
    let words = fetch(&WORDS, &dir, &mut log)?;
    let lexique = fetch(&LEXIQUE, &dir, &mut log)?;
    log("Construction du dictionnaire");
    let text = merge(&words, &lexique)?;
    let tmp = dir.join(format!("{MERGED}.{}.part", std::process::id()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &merged)?;
    log(&format!(
        "Dictionnaire enregistré dans {}",
        merged.display()
    ));
    Ok(merged)
}

/// Télécharge une source et vérifie son empreinte ; renvoie son contenu.
fn fetch(src: &Source, dir: &Path, log: &mut impl FnMut(&str)) -> io::Result<String> {
    log(&format!("  {}", src.url));
    let tmp = dir.join(format!("download.{}.part", std::process::id()));
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args(["--retry", "2", "--connect-timeout", "30", "--output"])
        .arg(&tmp)
        .arg(src.url)
        .status()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("impossible de lancer curl ({e}) : installer curl ou passer --dict"),
            )
        });
    let result = status.and_then(|st| {
        if !st.success() {
            return Err(io::Error::other(format!(
                "échec du téléchargement de {}",
                src.url
            )));
        }
        let bytes = fs::read(&tmp)?;
        let got = hex(&sha256(&bytes));
        if got != src.sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "empreinte inattendue pour {} : {got} au lieu de {}",
                    src.url, src.sha256
                ),
            ));
        }
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    });
    let _ = fs::remove_file(&tmp);
    result
}

/// Fusionne la liste de formes (tableau JSON) et les fréquences de Lexique.
fn merge(words_json: &str, lexique_tsv: &str) -> io::Result<String> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut freq: BTreeMap<String, f64> = BTreeMap::new();
    for w in parse_json_strings(words_json).ok_or_else(|| bad("index.json illisible"))? {
        freq.entry(w).or_insert(-1.0);
    }
    let mut lines = lexique_tsv.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| *h == name)
            .ok_or_else(|| bad(&format!("colonne {name} absente de Lexique")))
    };
    let (ortho, films, books) = (col("ortho")?, col("freqfilms2")?, col("freqlivres")?);
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        let Some(w) = f.get(ortho).filter(|w| !w.is_empty()) else {
            continue;
        };
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        if let Some(e) = freq.get_mut(*w) {
            *e = e.max(0.0) + (num(films) + num(books)) / 2.0;
        }
    }
    let mut out = String::from(
        "# Généré par motscroises : an-array-of-french-words 2.0.0 (MIT), \
         fréquences Lexique 3.83 (CC BY-SA 4.0)\n",
    );
    for (w, f) in &freq {
        let _ = writeln!(out, "{w}\t{}", score(*f));
    }
    Ok(out)
}

/// Score d'un mot de fréquence `f` (occurrences par million) ; négatif
/// signifie absent de Lexique.
fn score(f: f64) -> u32 {
    if f < 0.0 {
        0
    } else {
        1 + (10.0 * (1.0 + 100.0 * f).log10()).round() as u32
    }
}

/// Lit un tableau JSON de chaînes : `["a", "bé", ...]`.
fn parse_json_strings(s: &str) -> Option<Vec<String>> {
    crate::json::parse(s)?
        .as_arr()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// SHA-256 (FIPS 180-4).
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (chunk, v) in out.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(&sha256(long)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn json_strings() {
        let v = parse_json_strings(r#"["a", "bé" ,"\"q\"", "😀"]"#).unwrap();
        assert_eq!(v, vec!["a", "bé", "\"q\"", "😀"]);
        assert_eq!(parse_json_strings("[]").unwrap(), Vec::<String>::new());
        assert!(parse_json_strings("{}").is_none());
    }

    #[test]
    fn merge_scores() {
        let json = r#"["chat", "abaissasse"]"#;
        let tsv = "ortho\tfreqfilms2\tfreqlivres\nchat\t30\t50\nchat\t2\t0\nzut\t0\t0\n";
        let out = merge(json, tsv).unwrap();
        let lines: Vec<&str> = out.lines().skip(1).collect();
        // « zut », absent de la liste de formes, n'est pas admis.
        assert_eq!(lines, vec!["abaissasse\t0", "chat\t37"]);
    }
}
