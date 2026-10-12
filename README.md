# motscroises

Générateur de grilles de **mots croisés à la française** (cases noires, pas de symétrie imposée) et de **mots fléchés** (cases définitions, flèches droites et coudées), en Rust, sans aucune dépendance.

- Remplissage par CSP : lettres possibles par case et mots candidats par emplacement, propagés jusqu'au point fixe (cohérence d'arc entre mots croisés) ; recherche en profondeur sur l'emplacement le plus contraint, mot le moins contraignant d'abord ; aucun mot répété.
- Budget de nœuds court par gabarit et nouveaux tirages : certains gabarits sont faciles, d'autres presque impossibles, mieux vaut en changer que s'acharner.
- Candidats tenus en bitsets indexés par (position, lettre) : pas de parcours de liste de mots dans la boucle chaude.
- Liste de mots française téléchargée automatiquement au premier lancement, avec fréquences d'usage pour écarter les mots rares.
- Mots fléchés en quatre niveaux de difficulté (vocabulaire, densité de cases définitions, style des définitions), définitions dans les cases ou numérotées sous la grille, rédaction automatique des définitions manquantes par l'API Claude.
- Déterministe : même dictionnaire, mêmes options, même graine, même grille (le budget est compté en nœuds, pas en temps).
- Sorties texte (rangées en chiffres romains, colonnes en chiffres arabes) et SVG imprimable (grille vierge et solution).

## Dictionnaire

Sans `--dict`, la liste française est téléchargée au premier lancement (une trentaine de secondes, une seule fois) puis lue depuis le cache (`~/.cache/motscroises/`, ou `$XDG_CACHE_HOME`, `%LOCALAPPDATA%` sous Windows, ou `$MOTSCROISES_CACHE`). Elle fusionne deux sources, figées par version et vérifiées par empreinte SHA-256 :

- [an-array-of-french-words](https://github.com/words/an-array-of-french-words) 2.0.0 (MIT) : 336 000 formes, flexions comprises ;
- [Lexique](http://www.lexique.org) 3.83 (CC BY-SA 4.0) : fréquences d'usage dans les films et les livres.

La liste de formes fixe les mots admis ; Lexique ne fait que les noter (ses mots absents de la liste sont surtout des scories de sous-titres : « dano », « ok », « to »). Chaque mot reçoit un score qui croît avec le logarithme de sa fréquence : 0 pour un mot absent de Lexique, 12 vers 0,1 occurrence par million, 21 vers une par million, 41 vers cent par million. Le seuil `--min-score` vaut 12 par défaut pour les mots croisés (66 500 mots) et dépend du niveau pour les mots fléchés (voir plus bas) ; `--min-score 20` ne garde que les mots courants (31 000), `--min-score 0` toutes les formes (319 000 après normalisation). Les mots de meilleur score sont aussi favorisés au tirage. Le téléchargement passe par `curl` (présent sur Linux, macOS et Windows 10 et plus).

On peut aussi fournir sa propre liste avec `--dict` : un mot par ligne, avec un score entier optionnel après une tabulation (ou espace, `;`, `,`). Les accents et ligatures sont retirés à la lecture (`cœur` devient `COEUR`), les mots contenant un tiret, une apostrophe ou un chiffre sont ignorés, les doublons fusionnés. Aucun seuil n'est alors appliqué par défaut.

## CLI

```sh
cargo build --release

# Mots croisés 10x10
motscroises --size 10x10 --seed 1 --svg grille.svg --svg-solution solution.svg

# Remplir un gabarit (# noire, . vide, lettre imposée)
motscroises --template gabarit.txt

# Mots fléchés 9x11, niveau facile, avec définitions optionnelles
motscroises --fleches --size 9x11 --niveau 1 --defs defs.txt --svg fleches.svg --missing

# Mots fléchés difficiles à numéros (définitions sous la grille), définitions rédigées automatiquement
motscroises --fleches --size 11x13 --niveau 4 --numeros --auto-defs --svg fleches.svg

# Avec sa propre liste
motscroises --dict fr.txt --size 10x10
```

Les définitions sont au format `MOT<TAB>définition`, suivi éventuellement d'une tabulation et d'un niveau de 1 à 4 ; sans niveau, la définition vaut pour tous les niveaux, et une définition du niveau demandé prime. Plusieurs lignes sont possibles pour un même mot. Le cache de définitions, `definitions.txt` dans le dossier de cache, est toujours lu ; un fichier passé par `--defs` le complète et prime sur lui, sans jamais être modifié. Sans définition, la case affiche la longueur du mot ; `--missing` liste les mots restant à définir.

Avec `--numeros`, les cases définitions ne portent qu'un numéro (et leurs flèches), et les définitions numérotées, en ordre de lecture, sont listées sous la grille sur deux colonnes : pratique pour des définitions plus longues que ce qu'une case peut contenir.

### Niveaux de difficulté

`--niveau N` (mots fléchés, 2 par défaut) règle trois choses ; `--min-score` et `--blacks` restent prioritaires.

| Niveau | Seuil de fréquence | Préférence de vocabulaire | Cases définitions | Style des définitions |
|---|---|---|---|---|
| 1, facile | 22 (24 000 mots) | mots courants d'abord | +10 % | synonyme direct (« Saison chaude ») |
| 2, moyen | 17 (43 000 mots) | légère | référence | moins immédiat, sens second (« Petite baie ») |
| 3, difficile | 12 (66 500 mots) | aucune | -8 % | périphrase, référence culturelle (« Allegri le mit en musique ») |
| 4, très difficile | 8 (96 500 mots) | mots rares d'abord | -15 % | double sens, jeu de mots (« Encadré dans la presse » : OURS) |

Mesuré sur cinq grilles 11x13 par niveau : le score médian des mots passe de 34 au niveau 1 à 22 au niveau 4, la part de mots de score inférieur à 20 de 0 % à 44 %, la longueur moyenne des mots de 4,4 à 5,2 lettres. La génération reste sous la seconde.

### Définitions automatiques

Avec `--auto-defs`, les mots fléchés sans définition pour le niveau demandé sont envoyés en un seul lot à l'API Claude, avec la clé de `ANTHROPIC_API_KEY` (et `ANTHROPIC_BASE_URL` si elle est définie) : Claude Haiku 5.5 (`claude-haiku-5-5`) aux niveaux 1 et 2, Claude Opus 5.5 (`claude-opus-5-5`) aux niveaux 3 et 4, où Haiku ne produit pas de vrais jeux de mots. Les requêtes Opus activent le repli côté serveur (`fallbacks: "default"`) en cas de refus des filtres de sécurité.

Le modèle analyse d'abord chaque mot (graphie accentuée, nature, personne et temps) puis rédige une définition de même forme, au style du niveau, en s'inspirant d'un échantillon des définitions de ce niveau déjà présentes. Chaque définition est contrôlée : 30 caractères au plus, aucun mot de plus de 11 lettres (pour tenir dans une case), pas le mot à trouver. Les refusées sont redemandées une fois, avec la raison du refus. Les définitions acceptées sont ajoutées à la fin du cache de définitions avec leur niveau, sous un commentaire `# Rédigées par <modèle>, niveau N : à relire`, et resservent ensuite sans nouvel appel : seuls les mots absents du cache et de `--defs` pour ce niveau sont envoyés. Pour une grille 11x13 (une quarantaine de mots, jetons affichés à la fin) : moins d'un centime avec Haiku, une dizaine de centimes avec Opus. La clé est transmise à `curl` par l'entrée standard, jamais en argument de ligne de commande.

La relecture reste utile : sur des grilles de test, environ une définition sur vingt était discutable (sens peu naturel, forme approximative), un peu plus aux niveaux élevés où le jeu de mots peut tomber à plat.

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
use motscroises::{Dictionary, GenerateOptions, FlechesOptions, generate, generate_fleches, wordlist};

let path = wordlist::french(|msg| eprintln!("{msg}"))?; // téléchargée au premier appel
let dict = Dictionary::from_reader(std::io::BufReader::new(std::fs::File::open(path)?), 12)?;

let mut o = GenerateOptions::new(10, 10);
o.seed = 42;
let (grille, stats) = generate(&dict, &o)?;
println!("{grille}\n{}", grille.word_list());

let (fleches, _) = generate_fleches(&dict, &FlechesOptions::new(9, 11))?;
for clue in &fleches.clues {
    println!("{:?} -> {}", clue.arrow, fleches.solution.entries[clue.entry].word);
}
```

Points d'entrée : `generate` / `fill` (mots croisés), `generate_fleches` / `fill_fleches` (mots fléchés), `random_pattern` / `random_fleches_pattern` (gabarits seuls), `assign_clues` (rattachement mots/cases définitions), `Fleches::to_svg` / `to_svg_numbered` (rendus, définitions dans les cases ou numérotées sous la grille), `wordlist::french` (liste de mots par défaut), `autodefs::generate` (définitions par Claude Haiku 5.5).

## Règles des gabarits

**Mots croisés.** Cases blanches d'un seul tenant, aucune case blanche hors de tout mot, noires non adjacentes par défaut (`--adjacent` pour lever), longueur maximale des mots bornée, nombre de cases « muettes » dans un sens plafonné. Les noires sont placées en priorité là où elles coupent les plus longues suites blanches.

**Mots fléchés.** Première rangée et première colonne alternent cases définitions et cases lettres. Un mot est défini depuis la case qui le précède (flèche droite) ou, s'il part d'un bord, depuis la case voisine par une flèche coudée (`↴` vers la droite puis le bas, `↳` vers le bas puis la droite). Au plus deux définitions par case, aucune case définition inutile.

## Performances mesurées

Sur un cœur, graines 1 à 5, toutes réussies (chargement du dictionnaire : environ 0,1 s, non compté) :

| Grille | Taux de noires | Seuil | Temps typique |
|---|---|---|---|
| Mots croisés 8x8 à 15x15 | défaut (12 % à 20 %) | 12 | < 0,1 s |
| Mots croisés 13x13 | 14 % | 12 | 0,2 à 2 s |
| Mots croisés 10x10 | 10 % | 12 | 5 à 30 s |
| Mots croisés 10x10 | 10 % | 0 | 0,01 à 2 s |
| Mots croisés 10x10 | 8 % | 0 | 0,3 à 40 s |
| Mots croisés 13x13 | 12 % | 0 | 0,6 à 11 s |
| Mots croisés 15x15 | 13 % | 0 | 1 à 22 s |
| Mots fléchés 9x11 à 15x17 | défaut (20 % à 22 % au total) | 12 ou 20 | < 2 s |

Avant la propagation de contraintes, le 10x10 à 10 % prenait 4 à 40 s avec des échecs, et le 13x13 à 12 % échouait presque toujours. Les grilles très denses gagnent à baisser le seuil (plus de mots) ou la longueur maximale (`--max-len`).

## Tests

```sh
cargo test
```

39 tests unitaires et un doctest. Les grilles produites ont aussi été vérifiées par un script indépendant (60 grilles de 7x7 à 13x15, mots croisés et fléchés) : tous les mots appartiennent au dictionnaire, aucun doublon, aucune case orpheline, chaque mot fléché a sa case définition.

## Licence

MIT OR Apache-2.0.
