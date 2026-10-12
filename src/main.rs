use motscroises::{
    Definitions, Dictionary, FillOptions, Fleches, FlechesOptions, GenerateOptions, Grid, LEVELS,
    Solution, autodefs, fill, fill_fleches, generate, generate_fleches, level_settings, wordlist,
};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
motscroises : génère des grilles de mots croisés à la française
et de mots fléchés

USAGE
  motscroises [--size LxH | --template FICHIER] [options]
  motscroises --fleches [--size LxH | --template FICHIER] [options]

ENTRÉES
  --dict FICHIER       Dictionnaire : un mot par ligne, score optionnel après
                       une tabulation (les accents sont retirés à la lecture).
                       Par défaut, une liste française avec fréquences est
                       téléchargée au premier lancement puis mise en cache
  --size LxH           Taille de la grille à générer (défaut : 10x10)
  --template FICHIER   Gabarit à remplir (# noire ou case définition,
                       . vide, lettre imposée)
  --fleches            Mots fléchés au lieu de mots croisés
  --defs FICHIER       Définitions pour les mots fléchés : MOT<TAB>définition.
                       Elles priment sur le cache de définitions
                       (definitions.txt du dossier de cache), toujours lu
  --auto-defs          Mots fléchés : fait rédiger les définitions absentes
                       (Claude Haiku 5.5 aux niveaux 1 et 2, Opus 5.5 aux
                       niveaux 3 et 4 ; clé ANTHROPIC_API_KEY) et les
                       enregistre dans le cache de définitions, à relire
  --numeros            Mots fléchés : numéros dans les cases définitions,
                       définitions numérotées sous la grille
  --niveau N           Mots fléchés : difficulté de 1 (facile) à 4 (très
                       difficile), défaut 2. Règle le vocabulaire, la densité
                       de cases définitions et le style des définitions
                       rédigées par --auto-defs ; --min-score et --blacks
                       restent prioritaires

OPTIONS
  --seed N             Graine (défaut : tirée de l'horloge, affichée)
  --blacks R           Taux de cases noires visé, ex. 0.15 (génération) ;
                       en mots fléchés, taux de cases définitions intérieures
  --max-len N          Longueur maximale des mots (génération)
  --adjacent           Autorise les cases noires adjacentes (génération)
  --attempts N         Nombre de gabarits ou de redémarrages
  --budget N           Nœuds de recherche par tentative
  --min-score N        Ignore les mots de score inférieur (liste par défaut :
                       12, soit environ 0,1 occurrence par million de mots ;
                       0 garde toutes les formes, 20 les mots courants)
  --svg FICHIER        Écrit la grille vierge en SVG
  --svg-solution FICH  Écrit la grille remplie en SVG
  --missing            Mots fléchés : liste les mots sans définition
  -h, --help           Affiche cette aide
";

/// Seuil de fréquence appliqué par défaut à la liste téléchargée.
const DEFAULT_MIN_SCORE: u32 = 12;

/// Niveau des mots fléchés sans --niveau.
const DEFAULT_LEVEL: u8 = 2;

struct Args {
    dict: Option<String>,
    size: (usize, usize),
    template: Option<String>,
    seed: Option<u64>,
    blacks: Option<f64>,
    max_len: Option<usize>,
    adjacent: bool,
    attempts: Option<u32>,
    budget: Option<u64>,
    min_score: Option<u32>,
    svg: Option<String>,
    svg_solution: Option<String>,
    fleches: bool,
    defs: Option<String>,
    missing: bool,
    auto_defs: bool,
    numbered: bool,
    level: Option<u8>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        dict: None,
        size: (10, 10),
        template: None,
        seed: None,
        blacks: None,
        max_len: None,
        adjacent: false,
        attempts: None,
        budget: None,
        min_score: None,
        svg: None,
        svg_solution: None,
        fleches: false,
        defs: None,
        missing: false,
        auto_defs: false,
        numbered: false,
        level: None,
    };
    let mut it = std::env::args().skip(1);
    fn val<T: std::str::FromStr>(
        it: &mut impl Iterator<Item = String>,
        flag: &str,
    ) -> Result<T, String> {
        let v = it
            .next()
            .ok_or_else(|| format!("{flag} attend une valeur"))?;
        v.parse()
            .map_err(|_| format!("valeur invalide pour {flag} : {v}"))
    }
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--dict" => a.dict = Some(val(&mut it, &arg)?),
            "--size" => {
                let v: String = val(&mut it, &arg)?;
                let (w, h) = v
                    .split_once(['x', 'X', '*'])
                    .ok_or_else(|| format!("taille invalide : {v} (attendu LxH)"))?;
                a.size = (
                    w.parse().map_err(|_| format!("largeur invalide : {w}"))?,
                    h.parse().map_err(|_| format!("hauteur invalide : {h}"))?,
                );
            }
            "--template" => a.template = Some(val(&mut it, &arg)?),
            "--seed" => a.seed = Some(val(&mut it, &arg)?),
            "--blacks" => a.blacks = Some(val(&mut it, &arg)?),
            "--max-len" => a.max_len = Some(val(&mut it, &arg)?),
            "--adjacent" => a.adjacent = true,
            "--attempts" => a.attempts = Some(val(&mut it, &arg)?),
            "--budget" => a.budget = Some(val(&mut it, &arg)?),
            "--min-score" => a.min_score = Some(val(&mut it, &arg)?),
            "--svg" => a.svg = Some(val(&mut it, &arg)?),
            "--svg-solution" => a.svg_solution = Some(val(&mut it, &arg)?),
            "--fleches" | "--fléchés" => a.fleches = true,
            "--defs" => a.defs = Some(val(&mut it, &arg)?),
            "--missing" => a.missing = true,
            "--auto-defs" => a.auto_defs = true,
            "--numeros" | "--numéros" => a.numbered = true,
            "--niveau" | "--level" => {
                let n: u8 = val(&mut it, &arg)?;
                if !LEVELS.contains(&n) {
                    return Err(format!("niveau invalide : {n} (attendu 1 à 4)"));
                }
                a.level = Some(n);
            }
            other => return Err(format!("argument inconnu : {other}")),
        }
    }
    if a.auto_defs && !a.fleches {
        return Err("--auto-defs ne sert qu'avec --fleches".into());
    }
    if a.numbered && !a.fleches {
        return Err("--numeros ne sert qu'avec --fleches".into());
    }
    if a.level.is_some() && !a.fleches {
        return Err("--niveau ne sert qu'avec --fleches".into());
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let level = args.level.unwrap_or(DEFAULT_LEVEL);
    let settings = level_settings(level);
    let (path, min_score) = match &args.dict {
        Some(p) => (p.into(), args.min_score.unwrap_or(0)),
        None => (
            wordlist::french(|m| eprintln!("{m}")).map_err(|e| e.to_string())?,
            args.min_score.unwrap_or(if args.fleches {
                settings.min_score
            } else {
                DEFAULT_MIN_SCORE
            }),
        ),
    };
    let t0 = Instant::now();
    let file = File::open(&path).map_err(|e| format!("{} : {e}", path.display()))?;
    let dict =
        Dictionary::from_reader(BufReader::new(file), min_score).map_err(|e| e.to_string())?;
    eprintln!("Dictionnaire : {dict} chargés en {:.2?}", t0.elapsed());
    if args.fleches {
        eprintln!("Niveau : {level}");
    }

    let seed = args.seed.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    });
    eprintln!("Graine : {seed}");

    let template = match &args.template {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path} : {e}"))?;
            Some(Grid::parse(&text).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let fill_opts = {
        let mut o = FillOptions {
            seed,
            ..Default::default()
        };
        if args.fleches {
            o.score_weight = settings.score_weight;
        }
        if let Some(n) = args.attempts {
            o.attempts = n;
        }
        if let Some(n) = args.budget {
            o.node_budget = n;
        }
        o
    };

    let t1 = Instant::now();
    if args.fleches {
        // Les définitions de --defs priment ; le cache complète et reçoit
        // celles que rédige --auto-defs.
        let cache_path = wordlist::cache_dir().map(|d| d.join("definitions.txt"));
        let mut all = match &args.defs {
            Some(p) => load_definitions(Path::new(p))?,
            None => Definitions::default(),
        };
        if let Some(p) = cache_path.as_ref().filter(|p| p.is_file()) {
            all.extend_missing(load_definitions(p)?);
        }
        let mut defs = all.at_level(level);
        let fl = if let Some(grid) = &template {
            let fl = fill_fleches(&dict, grid, &fill_opts).map_err(|e| e.to_string())?;
            eprintln!("Remplie en {:.2?}", t1.elapsed());
            fl
        } else {
            let (w, h) = args.size;
            let mut o = FlechesOptions::new(w, h).with_level(level);
            o.seed = seed;
            if let Some(r) = args.blacks {
                o.inner_ratio = r;
            }
            if let Some(n) = args.max_len {
                o.max_word_len = n;
            }
            if let Some(n) = args.attempts {
                o.attempts = n;
            }
            if let Some(n) = args.budget {
                o.node_budget = n;
            }
            let (fl, stats) = generate_fleches(&dict, &o).map_err(|e| e.to_string())?;
            eprintln!(
                "Générée en {:.2?} (gabarit n° {}, {} nœuds)",
                t1.elapsed(),
                stats.attempt,
                stats.nodes
            );
            fl
        };
        if args.auto_defs {
            let missing = fl.missing_definitions(Some(&defs));
            if !missing.is_empty() {
                let path =
                    cache_path.ok_or("aucun dossier de cache : définir MOTSCROISES_CACHE")?;
                let r = autodefs::generate(&missing, &defs, level, |m| eprintln!("{m}"))
                    .map_err(|e| e.to_string())?;
                append_definitions(&path, &r.definitions, level, &r.model)?;
                for (w, def) in &r.definitions {
                    defs.insert_level(w, def, level);
                }
                eprintln!(
                    "{} définition(s) ajoutée(s) à {}, à relire ({} jetons en entrée, {} en sortie)",
                    r.definitions.len(),
                    path.display(),
                    r.input_tokens,
                    r.output_tokens
                );
                if !r.rejected.is_empty() {
                    eprintln!("Sans définition valable :\n  {}", r.rejected.join("\n  "));
                }
            }
        }
        report_fleches(&fl, Some(&defs), &args)
    } else {
        let solution = if let Some(grid) = &template {
            let sol = fill(&dict, grid, &fill_opts).map_err(|e| e.to_string())?;
            eprintln!("Remplie en {:.2?}", t1.elapsed());
            sol
        } else {
            let (w, h) = args.size;
            let mut o = GenerateOptions::new(w, h);
            o.seed = seed;
            o.pattern.allow_adjacent = args.adjacent;
            if let Some(r) = args.blacks {
                o.pattern.black_ratio = r;
            }
            if let Some(n) = args.max_len {
                o.pattern.max_word_len = n;
            }
            if let Some(n) = args.attempts {
                o.attempts = n;
            }
            if let Some(n) = args.budget {
                o.node_budget = n;
            }
            let (sol, stats) = generate(&dict, &o).map_err(|e| e.to_string())?;
            eprintln!(
                "Générée en {:.2?} (gabarit n° {}, {} nœuds)",
                t1.elapsed(),
                stats.attempt,
                stats.nodes
            );
            sol
        };
        report_crossword(&solution, &args)
    }
}

fn stats_line(sol: &Solution, what: &str) {
    let blocks = sol.grid.block_count();
    let cells = sol.grid.width() * sol.grid.height();
    eprintln!(
        "{} mots, {blocks} {what} ({:.0} %)",
        sol.entries.len(),
        100.0 * blocks as f64 / cells as f64
    );
}

fn load_definitions(path: &Path) -> Result<Definitions, String> {
    let f = File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    Definitions::from_reader(BufReader::new(f)).map_err(|e| format!("{} : {e}", path.display()))
}

/// Ajoute des définitions générées à la fin du fichier, sous un en-tête qui
/// les signale à la relecture.
fn append_definitions(
    path: &Path,
    defs: &[(String, String)],
    level: u8,
    model: &str,
) -> Result<(), String> {
    if defs.is_empty() {
        return Ok(());
    }
    let err = |e: std::io::Error| format!("{} : {e}", path.display());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let mut text = format!("# Rédigées par {model}, niveau {level} : à relire\n");
    for (w, d) in defs {
        text.push_str(&format!("{w}\t{d}\t{level}\n"));
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(err)
}

fn write_file(path: &str, content: String) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("{path} : {e}"))
}

fn report_crossword(solution: &Solution, args: &Args) -> Result<(), String> {
    println!("{solution}");
    print!("{}", solution.word_list());
    stats_line(solution, "cases noires");
    if let Some(p) = &args.svg {
        write_file(p, blank(&solution.grid).to_svg(false))?;
    }
    if let Some(p) = &args.svg_solution {
        write_file(p, solution.grid.to_svg(true))?;
    }
    Ok(())
}

fn report_fleches(fl: &Fleches, defs: Option<&Definitions>, args: &Args) -> Result<(), String> {
    println!("{}", fl.solution);
    if args.numbered {
        println!("Définitions (numéro, flèche, mot)");
        print!("{}", fl.numbered_clue_list(defs));
    } else {
        println!("Définitions (case, flèche, mot)");
        print!("{}", fl.clue_list(defs));
    }
    stats_line(&fl.solution, "cases définitions");
    if args.missing {
        println!("Mots sans définition");
        for w in fl.missing_definitions(defs) {
            println!("{w}");
        }
    }
    let svg = |f: &Fleches, letters: bool| {
        if args.numbered {
            f.to_svg_numbered(letters, defs)
        } else {
            f.to_svg(letters, defs)
        }
    };
    if let Some(p) = &args.svg {
        let mut b = fl.clone();
        b.solution.grid = blank(&fl.solution.grid);
        write_file(p, svg(&b, false))?;
    }
    if let Some(p) = &args.svg_solution {
        write_file(p, svg(fl, true))?;
    }
    Ok(())
}

/// Copie de la grille sans ses lettres.
fn blank(g: &Grid) -> Grid {
    let mut b = g.clone();
    for r in 0..b.height() {
        for c in 0..b.width() {
            if !b.get(r, c).is_block() {
                b.set(r, c, motscroises::Cell::Open(None));
            }
        }
    }
    b
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}
