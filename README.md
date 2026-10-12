# motscroises

Générateur de grilles de **mots croisés à la française** (cases noires, pas de symétrie imposée) et de **mots fléchés** (cases définitions, flèches droites et coudées), en Rust, sans aucune dépendance.

- Remplissage par CSP : recherche en profondeur, heuristique MRV (emplacement le plus contraint d'abord), vérification en avant sur les mots croisés, aucun mot répété, redémarrages aléatoires sous budget de nœuds.
- Candidats comptés par ET de bitsets indexés par (position, lettre) : pas de parcours de liste de mots dans la boucle chaude.
- Déterministe : même dictionnaire, mêmes options, même graine, même grille (le budget est compté en nœuds, pas en temps).
- Sorties texte (rangées en chiffres romains, colonnes en chiffres arabes) et SVG imprimable (grille vierge et solution).

## Dictionnaire

La crate n'embarque pas de dictionnaire. Le format attendu est un mot par ligne, avec un score entier optionnel après une tabulation (ou espace, `;`, `,`). Les accents et ligatures sont retirés à la lecture (`cœur` devient `COEUR`), les mots contenant un tiret, une apostrophe ou un chiffre sont ignorés, les doublons fusionnés.

Testé avec la liste MIT [an-array-of-french-words](https://github.com/words/an-array-of-french-words) (336 000 entrées, 318 881 mots après normalisation, flexions comprises) :

```sh
npm pack an-array-of-french-words && tar xzf an-array-of-french-words-*.tgz
python3 -c "import json;print('\n'.join(json.load(open('package/index.json'))))" > fr.txt
```

Cette liste contient abréviations et unités (`CL`, `KG`, `KM`), ce qui est d'usage en mots croisés, mais aussi des formes rares. Pour des grilles plus « grand public », fournir une liste avec scores (fréquences Lexique par exemple) et filtrer avec `--min-score`, ou laisser les scores orienter le tirage (les mots de meilleur score sont favorisés).

## CLI

```sh
cargo build --release

# Mots croisés 10x10
motscroises --dict fr.txt --size 10x10 --seed 1 --svg grille.svg --svg-solution solution.svg

# Remplir un gabarit (# noire, . vide, lettre imposée)
motscroises --dict fr.txt --template gabarit.txt

# Mots fléchés 9x11, avec définitions optionnelles
motscroises --fleches --dict fr.txt --size 9x11 --defs defs.txt --svg fleches.svg --missing
```

Le fichier de définitions est au format `MOT<TAB>définition` (plusieurs lignes possibles pour un même mot). Sans définition, la case affiche la longueur du mot ; `--missing` liste les mots restant à définir.

Exemple de sortie :

```
        1  2  3  4  5  6  7  8  9 10
    I   D  R  O  L  E  S  S  E  S  ■
   II   E  U  ■  A  M  U  I  ■  C  L
  III   ■  A  R  D  O  I  S  E  R  A
  ...
Horizontalement
      I. DROLESSES
     II. EU - AMUI - CL
  ...
```

## Bibliothèque

```rust
use motscroises::{Dictionary, GenerateOptions, FlechesOptions, generate, generate_fleches};

let dict = Dictionary::from_reader(std::io::BufReader::new(std::fs::File::open("fr.txt")?), 0)?;

let mut o = GenerateOptions::new(10, 10);
o.seed = 42;
let (grille, stats) = generate(&dict, &o)?;
println!("{grille}\n{}", grille.word_list());

let (fleches, _) = generate_fleches(&dict, &FlechesOptions::new(9, 11))?;
for clue in &fleches.clues {
    println!("{:?} -> {}", clue.arrow, fleches.solution.entries[clue.entry].word);
}
```

Points d'entrée : `generate` / `fill` (mots croisés), `generate_fleches` / `fill_fleches` (mots fléchés), `random_pattern` / `random_fleches_pattern` (gabarits seuls), `assign_clues` (rattachement mots/cases définitions).

## Règles des gabarits

**Mots croisés.** Cases blanches d'un seul tenant, aucune case blanche hors de tout mot, noires non adjacentes par défaut (`--adjacent` pour lever), longueur maximale des mots bornée, nombre de cases « muettes » dans un sens plafonné. Les noires sont placées en priorité là où elles coupent les plus longues suites blanches.

**Mots fléchés.** Première rangée et première colonne alternent cases définitions et cases lettres. Un mot est défini depuis la case qui le précède (flèche droite) ou, s'il part d'un bord, depuis la case voisine par une flèche coudée (`↴` vers la droite puis le bas, `↳` vers le bas puis la droite). Au plus deux définitions par case, aucune case définition inutile.

## Performances mesurées

Sur un cœur, dictionnaire de 318 881 mots (chargement : environ 0,4 s, non compté) :

| Grille | Taux de noires | Temps typique |
|---|---|---|
| Mots croisés 8x8 à 15x15 | défaut (12 % à 20 %) | < 0,5 s |
| Mots croisés 10x10 | 12 % | 1 à 6 s |
| Mots croisés 10x10 | 10 % | 4 à 40 s, échecs possibles |
| Mots croisés 13x13 | 12 % | échec dans le budget par défaut |
| Mots fléchés 9x11 à 15x17 | défaut (20 % à 22 % au total) | < 2 s |

Le taux par défaut croît avec la taille. En dessous, la difficulté explose : c'est la limite d'une recherche chronologique avec ce dictionnaire. Pistes d'amélioration : backjumping dirigé par les conflits, placement dynamique des noires pendant le remplissage, choix des valeurs les moins contraignantes.

## Tests

```sh
cargo test
```

27 tests unitaires et un doctest. Les grilles produites ont aussi été vérifiées par un script indépendant (50 grilles de 7x7 à 13x13, mots croisés et fléchés) : tous les mots appartiennent au dictionnaire, aucun doublon, aucune case orpheline, chaque mot fléché a sa case définition.

## Licence

MIT OR Apache-2.0.
