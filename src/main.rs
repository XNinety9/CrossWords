use motscroises::{
    Definitions, Dictionary, FillOptions, Fleches, FlechesOptions, GenerateOptions, Grid, Solution,
    fill, fill_fleches, generate, generate_fleches,
};
use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
motscroises : génère des grilles de mots croisés à la française
et de mots fléchés

USAGE
  motscroises --dict FICHIER [--size LxH | --template FICHIER] [options]
  motscroises --fleches --dict FICHIER [--size LxH | --template FICHIER] [options]

ENTRÉES
  --dict FICHIER       Dictionnaire : un mot par ligne, score optionnel après
                       une tabulation (les accents sont retirés à la lecture)
  --size LxH           Taille de la grille à générer (défaut : 10x10)
  --template FICHIER   Gabarit à remplir (# noire ou case définition,
                       . vide, lettre imposée)
  --fleches            Mots fléchés au lieu de mots croisés
  --defs FICHIER       Définitions pour les mots fléchés : MOT<TAB>définition

OPTIONS
  --seed N             Graine (défaut : tirée de l'horloge, affichée)
  --blacks R           Taux de cases noires visé, ex. 0.15 (génération) ;
                       en mots fléchés, taux de cases définitions intérieures
  --max-len N          Longueur maximale des mots (génération)
  --adjacent           Autorise les cases noires adjacentes (génération)
  --attempts N         Nombre de gabarits ou de redémarrages
  --budget N           Nœuds de recherche par tentative
  --min-score N        Ignore les mots de score inférieur
  --svg FICHIER        Écrit la grille vierge en SVG
  --svg-solution FICH  Écrit la grille remplie en SVG
  --missing            Mots fléchés : liste les mots sans définition
  -h, --help           Affiche cette aide
";

struct Args {
    dict: String,
    size: (usize, usize),
    template: Option<String>,
    seed: Option<u64>,
    blacks: Option<f64>,
    max_len: Option<usize>,
    adjacent: bool,
    attempts: Option<u32>,
    budget: Option<u64>,
    min_score: u32,
    svg: Option<String>,
    svg_solution: Option<String>,
    fleches: bool,
    defs: Option<String>,
    missing: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        dict: String::new(),
        size: (10, 10),
        template: None,
        seed: None,
        blacks: None,
        max_len: None,
        adjacent: false,
        attempts: None,
        budget: None,
        min_score: 0,
        svg: None,
        svg_solution: None,
        fleches: false,
        defs: None,
        missing: false,
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
            "--dict" => a.dict = val(&mut it, &arg)?,
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
            "--min-score" => a.min_score = val(&mut it, &arg)?,
            "--svg" => a.svg = Some(val(&mut it, &arg)?),
            "--svg-solution" => a.svg_solution = Some(val(&mut it, &arg)?),
            "--fleches" | "--fléchés" => a.fleches = true,
            "--defs" => a.defs = Some(val(&mut it, &arg)?),
            "--missing" => a.missing = true,
            other => return Err(format!("argument inconnu : {other}")),
        }
    }
    if a.dict.is_empty() {
        return Err("--dict est obligatoire".into());
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let t0 = Instant::now();
    let file = File::open(&args.dict).map_err(|e| format!("{} : {e}", args.dict))?;
    let dict =
        Dictionary::from_reader(BufReader::new(file), args.min_score).map_err(|e| e.to_string())?;
    eprintln!("Dictionnaire : {dict} chargés en {:.2?}", t0.elapsed());

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
        let defs = match &args.defs {
            Some(p) => {
                let f = File::open(p).map_err(|e| format!("{p} : {e}"))?;
                Some(Definitions::from_reader(BufReader::new(f)).map_err(|e| e.to_string())?)
            }
            None => None,
        };
        let fl = if let Some(grid) = &template {
            let fl = fill_fleches(&dict, grid, &fill_opts).map_err(|e| e.to_string())?;
            eprintln!("Remplie en {:.2?}", t1.elapsed());
            fl
        } else {
            let (w, h) = args.size;
            let mut o = FlechesOptions::new(w, h);
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
        report_fleches(&fl, defs.as_ref(), &args)
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
    println!("Définitions (case, flèche, mot)");
    print!("{}", fl.clue_list(defs));
    stats_line(&fl.solution, "cases définitions");
    if args.missing {
        println!("Mots sans définition");
        for w in fl.missing_definitions(defs) {
            println!("{w}");
        }
    }
    if let Some(p) = &args.svg {
        let mut b = fl.clone();
        b.solution.grid = blank(&fl.solution.grid);
        write_file(p, b.to_svg(false, defs))?;
    }
    if let Some(p) = &args.svg_solution {
        write_file(p, fl.to_svg(true, defs))?;
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
