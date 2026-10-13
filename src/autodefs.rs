//! Rédaction automatique des définitions de mots fléchés : Claude Haiku 5.5
//! pour les niveaux 1 et 2, Claude Opus 5.5 pour les niveaux 3 et 4 (jeux de
//! mots et références, que Haiku réussit mal).
//!
//! Les mots sans définition sont envoyés en lot à l'API Messages (clé lue
//! dans `ANTHROPIC_API_KEY`, passée à `curl` par l'entrée standard pour ne
//! jamais apparaître dans la liste des processus). La réponse suit un schéma
//! JSON imposé ; chaque définition est ensuite contrôlée (longueur, largeur
//! des mots pour tenir dans une case, mot-réponse absent) et les refusées
//! sont redemandées une fois.

use crate::Language;
use crate::dict::normalize;
use crate::fleches::Definitions;
use crate::json::{self, Json};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::process::{Command, Stdio};

/// Modèle des niveaux 1 et 2 : définitions directes, coût minimal.
pub const EASY_MODEL: &str = "claude-haiku-5-5";
/// Modèle des niveaux 3 et 4 : périphrases, références, doubles sens.
pub const HARD_MODEL: &str = "claude-opus-5-5";

/// Longueur maximale d'une définition, en caractères.
pub const MAX_CHARS: usize = 30;
/// Largeur maximale d'un mot de la définition (une ligne de case).
pub const MAX_WORD_CHARS: usize = 11;

/// Couples (mot, définition).
type Pairs = Vec<(String, String)>;

const BATCH: usize = 80;
const EXAMPLES: usize = 40;

const SYSTEM: &str = "Tu rédiges des définitions de mots fléchés en français.

Chaque mot t'est donné en majuscules, sans accents. Pour chacun, commence par l'identifier dans le champ « analyse » : sa graphie exacte avec accents, sa nature et sa forme (personne, temps, genre, nombre). Plusieurs mots peuvent s'écrire pareil sans accents : AGI est « agi », participe passé de agir ; AIT est « ait », subjonctif présent de avoir ; REGENT est « régent », mais aussi « régent », 3e personne du pluriel de régir ; ALE est « ale », bière anglaise ; AIE est « aïe », cri de douleur. Choisis la lecture la plus naturelle dans une grille.

Puis écris la définition, dans le style des mots fléchés et au niveau de difficulté indiqué plus bas :
- courte : 1 à 5 mots, 30 caractères au plus, aucun mot de plus de 11 lettres (elle doit tenir dans une petite case) ;
- exactement de la même forme que le mot analysé : un verbe conjugué se définit par un verbe à la même personne et au même temps (SERS : « Rends service », REGENT : « Gouvernent »), un participe par un participe, un pluriel par un pluriel, un féminin par un féminin ;
- sans le mot lui-même, ni un mot de la même famille, ni une variante orthographique (CLES : « Clefs » est interdit) ;
- en français naturel, pas en style télégraphique (PAIN : « Sort du fournil », pas « Boulangerie produit ») ;
- jamais dans un sens vulgaire ou sexuel : prends un autre sens, ou à défaut une indication neutre ;
- pour un nom propre, un mot étranger, une abréviation ou un mot grammatical, une indication brève (« Monsieur anglais », « Ville de la Drôme », « Pronom réfléchi ») ;
- avec majuscule initiale et sans point final.

Réponds pour chaque mot reçu, en recopiant le mot tel quel dans le champ « mot ».";

/// Résultat d'une rédaction.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// Définitions acceptées : (mot, définition).
    pub definitions: Vec<(String, String)>,
    /// Mots restés sans définition valable, avec la raison du dernier refus.
    pub rejected: Vec<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Modèle employé.
    pub model: String,
}

/// Rédige une définition française de niveau `level` (1 à 4) pour chaque
/// mot de `words`, dans le style de celles de ce niveau déjà présentes dans
/// `examples`.
pub fn generate(
    words: &[String],
    examples: &Definitions,
    level: u8,
    log: impl FnMut(&str),
) -> io::Result<Report> {
    generate_for(Language::Fr, words, examples, level, log)
}

/// Comme [`generate`], dans la langue `lang`.
pub fn generate_for(
    lang: Language,
    words: &[String],
    examples: &Definitions,
    level: u8,
    log: impl FnMut(&str),
) -> io::Result<Report> {
    run(model_for_level(level), lang, words, examples, level, log)
}

/// Modèle employé pour un niveau.
pub fn model_for_level(level: u8) -> &'static str {
    if level >= 3 { HARD_MODEL } else { EASY_MODEL }
}

/// Comme [`generate`], avec un modèle choisi.
pub fn generate_with(
    model: &str,
    words: &[String],
    examples: &Definitions,
    level: u8,
    log: impl FnMut(&str),
) -> io::Result<Report> {
    run(model, Language::Fr, words, examples, level, log)
}

fn run(
    model: &str,
    lang: Language,
    words: &[String],
    examples: &Definitions,
    level: u8,
    mut log: impl FnMut(&str),
) -> io::Result<Report> {
    let key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "ANTHROPIC_API_KEY n'est pas définie",
            )
        })?;
    let system = system_prompt(&examples.at_level(level), level, lang);
    let mut report = Report {
        model: model.to_string(),
        ..Report::default()
    };
    let mut todo: Vec<String> = words.to_vec();
    let mut feedback: Vec<String> = Vec::new();
    for round in 0..2 {
        if todo.is_empty() {
            break;
        }
        if round > 0 {
            log(&format!("Nouvel essai pour {} mot(s)", todo.len()));
        }
        let mut retry = Vec::new();
        let mut refusals = Vec::new();
        for chunk in todo.chunks(BATCH) {
            log(&format!(
                "Rédaction de {} définition(s) par {model}",
                chunk.len()
            ));
            let effort = if level >= 3 { "medium" } else { "low" };
            let (got, tin, tout) = call(&key, model, effort, &system, lang, chunk, &feedback)?;
            report.input_tokens += tin;
            report.output_tokens += tout;
            for w in chunk {
                let reason = match got.iter().find(|(m, _)| m == w) {
                    Some((_, d)) => match check(w, d) {
                        Ok(()) => {
                            report.definitions.push((w.clone(), d.clone()));
                            continue;
                        }
                        Err(why) => format!("{w} : « {d} » ({why})"),
                    },
                    None => format!("{w} : oublié"),
                };
                retry.push(w.clone());
                refusals.push(reason);
            }
        }
        todo = retry;
        feedback = refusals;
    }
    report.rejected = feedback;
    Ok(report)
}

/// Vérifie qu'une définition tient dans une case et ne livre pas la réponse.
pub fn check(word: &str, def: &str) -> Result<(), String> {
    let def = def.trim();
    if def.is_empty() {
        return Err("définition vide".into());
    }
    if def.chars().count() > MAX_CHARS {
        return Err(format!("plus de {MAX_CHARS} caractères"));
    }
    let target = normalize(word).unwrap_or_default();
    for token in def.split(|c: char| !c.is_alphabetic()) {
        if token.chars().count() > MAX_WORD_CHARS {
            return Err(format!("« {token} » trop long pour une case"));
        }
        let Some(t) = normalize(token) else { continue };
        let gives_away = if target.len() >= 4 {
            t.windows(target.len()).any(|w| w == target.as_slice())
        } else {
            t == target
        };
        if !target.is_empty() && gives_away {
            return Err("contient le mot à trouver".into());
        }
    }
    Ok(())
}

/// Style attendu à chaque niveau, avec des exemples.
fn level_style(level: u8) -> &'static str {
    match level.clamp(1, 4) {
        1 => {
            "Niveau 1 sur 4, facile : synonyme direct ou définition évidente, la réponse doit venir tout de suite. Exemples : ETE : « Saison chaude » ; SERS : « Rends service » ; CIME : « Sommet »."
        }
        2 => {
            "Niveau 2 sur 4, moyen : définition exacte mais moins immédiate, par un synonyme moins courant, une précision, un sens second ou une petite allusion. Exemples : ETE : « Saison des moissons » ; ANSE : « Petite baie » ; CIME : « Point culminant » ; ERRA : « Alla sans but »."
        }
        3 => {
            "Niveau 3 sur 4, difficile : périphrase indirecte, sens figuré ou secondaire, référence culturelle. La réponse demande de la réflexion, mais la définition reste exacte. Exemples : ETE : « Finit en septembre » ; TSAR : « Ivan le fut » ; ERRA : « Fit comme Ulysse » ; OR : « Vaut le silence »."
        }
        _ => {
            "Niveau 4 sur 4, très difficile : double sens, jeu de mots, fausse piste. La définition semble désigner autre chose et ne s'éclaire qu'une fois la réponse trouvée ; un point d'interrogation final peut signaler le jeu de mots. Elle doit rester juste pour le mot. Exemples : LIERRE : « Fait le mur » ; OR : « Mais précieux » ; ETE : « Participe au passé » ; PNEU : « Gonflé, il roule »."
        }
    }
}

/// Style attendu à chaque niveau pour l'anglais, avec des exemples.
fn level_style_en(level: u8) -> &'static str {
    match level.clamp(1, 4) {
        1 => {
            "Level 1 of 4, easy: a direct synonym or obvious definition; the answer should come at once. Examples: SUMMER: \"Warm season\"; SERVE: \"Wait on\"; PEAK: \"Summit\"."
        }
        2 => {
            "Level 2 of 4, medium: accurate but less immediate, through a less common synonym, a precision, a secondary meaning or a light allusion. Examples: COVE: \"Small bay\"; PEAK: \"Highest point\"; ROAMED: \"Wandered aimlessly\"; ERE: \"Before, poetically\"."
        }
        3 => {
            "Level 3 of 4, hard: indirect paraphrase, figurative or secondary meaning, cultural reference. The answer takes some thought, but the clue stays accurate. Examples: SUMMER: \"It ends in September\"; TSAR: \"Ivan, for one\"; ROAMED: \"Did as Odysseus did\"; GOLDEN: \"Like silence, proverbially\"."
        }
        _ => {
            "Level 4 of 4, very hard: double meaning, wordplay, misdirection. The clue seems to point elsewhere and only makes sense once the answer is found; a final question mark signals the wordplay, as in American crosswords. It must still be accurate for the answer. Examples: IVY: \"Wall climber\"; TEE: \"Driver's support?\"; ENVY: \"Green-eyed monster\"; BANK: \"Place with many branches?\"."
        }
    }
}

const SYSTEM_EN: &str = "You write clues for arrowword and crossword puzzles in English.

Each answer is given in capital letters, with no spaces or punctuation. For each one, start by identifying it in the \"analyse\" field: its usual spelling, part of speech and form (tense, person, number), and the reading that is most natural in a puzzle when several exist (LEAD is a metal or to guide; TEARS are drops or rips).

Then write the clue, in the style of crossword puzzles and at the difficulty level given below:
- short: 1 to 5 words, at most 30 characters, no word longer than 11 letters (it must fit in a small grid cell);
- exactly the same form as the answer: a plural answer takes a plural clue, a past tense answer a past tense clue (RAN: \"Sprinted\"), an adverb an adverb;
- never containing the answer itself, a word from the same family, or a variant spelling of it;
- in natural English with American spelling (like the word list), not telegraphic;
- never in a vulgar or sexual sense: pick another meaning, or else a neutral hint;
- for an abbreviation, a foreign word, a proper name or a function word, a brief pointer (\"Definite article\" for THE, \"Prefix with sphere\" for ATMO);
- with a capital first letter and no final period.

Answer for every word received, copying it exactly as given into the \"mot\" field.";

fn system_prompt(examples: &Definitions, level: u8, lang: Language) -> String {
    let entries = examples.entries();
    let (base, style, more) = match lang {
        Language::Fr => (
            SYSTEM,
            level_style(level),
            "Autres définitions de ce niveau, pour le ton :",
        ),
        Language::En => (
            SYSTEM_EN,
            level_style_en(level),
            "Other clues at this level, for tone:",
        ),
    };
    let mut s = String::from(base);
    s.push_str("\n\n");
    s.push_str(style);
    if entries.is_empty() {
        return s;
    }
    s.push_str("\n\n");
    s.push_str(more);
    s.push('\n');
    let step = entries.len().div_ceil(EXAMPLES).max(1);
    for (w, d) in entries.iter().step_by(step) {
        let _ = writeln!(s, "{w} : {d}");
    }
    s
}

const SCHEMA: &str = r#"{"type":"object","properties":{"definitions":{"type":"array","items":{"type":"object","properties":{"mot":{"type":"string"},"analyse":{"type":"string"},"definition":{"type":"string"}},"required":["mot","analyse","definition"],"additionalProperties":false}}},"required":["definitions"],"additionalProperties":false}"#;

/// Un appel à l'API ; renvoie les couples reçus et les jetons consommés.
fn call(
    key: &str,
    model: &str,
    effort: &str,
    system: &str,
    lang: Language,
    words: &[String],
    feedback: &[String],
) -> io::Result<(Pairs, u64, u64)> {
    let (head, retry) = match lang {
        Language::Fr => (
            "Mots à définir :",
            "Ces définitions ont été refusées au premier essai ; propose autre chose :",
        ),
        Language::En => (
            "Answers to clue:",
            "These clues were rejected on the first attempt (reasons in French); suggest something else:",
        ),
    };
    let mut user = format!("{head}\n{}", words.join("\n"));
    if !feedback.is_empty() {
        user.push_str("\n\n");
        user.push_str(retry);
        user.push('\n');
        user.push_str(&feedback.join("\n"));
    }
    // Repli côté serveur en cas de refus des filtres de sécurité, conseillé
    // pour Opus 5.5 ; Haiku 5.5 n'en a pas.
    let fallback = model.starts_with("claude-opus");
    let body = format!(
        r#"{{"model":{},"max_tokens":16000,{}"output_config":{{"effort":{},"format":{{"type":"json_schema","schema":{SCHEMA}}}}},"system":{},"messages":[{{"role":"user","content":{}}}]}}"#,
        json::quote(model),
        if fallback {
            r#""fallbacks":"default","#
        } else {
            ""
        },
        json::quote(effort),
        json::quote(system),
        json::quote(&user),
    );
    let base = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com".into());
    // Configuration curl passée par l'entrée standard : la clé n'apparaît
    // pas dans les arguments du processus.
    let mut config = format!(
        "url = {}\nsilent\nshow-error\nmax-time = 300\nheader = \"content-type: application/json\"\nheader = \"anthropic-version: 2023-06-01\"\nheader = {}\ndata-binary = {}\n",
        curl_quote(&format!("{}/v1/messages", base.trim_end_matches('/'))),
        curl_quote(&format!("x-api-key: {key}")),
        curl_quote(&body),
    );
    if fallback {
        config.push_str("header = \"anthropic-beta: server-side-fallback-2026-07-01\"\n");
    }
    let mut child = Command::new("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("impossible de lancer curl ({e})")))?;
    child
        .stdin
        .take()
        .expect("entrée standard redirigée")
        .write_all(config.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "échec de l'appel à l'API : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
    let resp = json::parse(&text).ok_or_else(|| bad("réponse de l'API illisible".into()))?;
    if let Some(err) = resp.get("error") {
        let msg = err.get("message").and_then(Json::as_str).unwrap_or("?");
        return Err(io::Error::other(format!("erreur de l'API : {msg}")));
    }
    match resp.get("stop_reason").and_then(Json::as_str) {
        Some("end_turn") => {}
        Some(r) => return Err(bad(format!("réponse interrompue ({r})"))),
        None => return Err(bad("réponse sans stop_reason".into())),
    }
    let usage = |k: &str| {
        resp.get("usage")
            .and_then(|u| u.get(k))
            .and_then(Json::as_f64)
            .unwrap_or(0.0) as u64
    };
    let payload = resp
        .get("content")
        .and_then(Json::as_arr)
        .and_then(|blocks| {
            blocks
                .iter()
                .find(|b| b.get("type").and_then(Json::as_str) == Some("text"))
        })
        .and_then(|b| b.get("text"))
        .and_then(Json::as_str)
        .and_then(json::parse)
        .ok_or_else(|| bad("réponse sans JSON exploitable".into()))?;
    let wanted: BTreeSet<&str> = words.iter().map(String::as_str).collect();
    let defs = payload
        .get("definitions")
        .and_then(Json::as_arr)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| {
            let m = e.get("mot")?.as_str()?.trim().to_uppercase();
            let d = e.get("definition")?.as_str()?.trim().to_string();
            wanted.contains(m.as_str()).then_some((m, d))
        })
        .collect();
    Ok((defs, usage("input_tokens"), usage("output_tokens")))
}

/// Chaîne entre guillemets pour un fichier de configuration curl.
fn curl_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        assert!(check("SERS", "Rends service").is_ok());
        assert!(check("ETE", "Saison chaude").is_ok());
        assert!(check("ETE", "").is_err());
        assert!(check("ETE", "Saison où il fait chaud, avec du soleil").is_err());
        assert!(check("TEST", "Phénoménalement court").is_err());
        assert!(check("PNEU", "Pneumatique").is_err());
        assert!(check("ET", "Petite conjonction").is_ok());
        assert!(check("ET", "Et voilà").is_err());
        assert!(check("ETE", "Été indien").is_err());
    }

    #[test]
    fn prompts_follow_language() {
        let mut ex = Definitions::default();
        ex.insert_level("COVE", "Small bay", 2);
        let en = system_prompt(&ex.at_level(2), 2, Language::En);
        assert!(en.starts_with("You write clues"));
        assert!(en.contains("Level 2 of 4") && en.contains("COVE : Small bay"));
        let fr = system_prompt(&Definitions::default(), 4, Language::Fr);
        assert!(fr.starts_with("Tu rédiges") && fr.contains("Niveau 4 sur 4"));
        assert!(!fr.contains("Autres définitions"));
    }

    #[test]
    fn curl_quoting() {
        assert_eq!(curl_quote(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(curl_quote("x\ny"), r#""x\ny""#);
    }
}
