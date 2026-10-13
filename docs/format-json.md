# Format JSON des grilles

CrossWords exporte ses grilles, mots croisés comme mots fléchés, dans un format JSON unique, pour les afficher ou les exploiter dans un autre programme (site web, application, mise en page).

- Option : `--json FICHIER`, ou `--json -` pour l'écrire sur la sortie standard à la place du rendu texte (les messages de progression restent sur la sortie d'erreur).
- Bibliothèque : `Solution::to_json` (mots croisés) et `Fleches::to_json` (mots fléchés).
- Schéma : [`crosswords.schema.json`](crosswords.schema.json) (JSON Schema 2020-12).
- Version du format : 1 (constante `JSON_FORMAT_VERSION`).

```sh
crosswords --fleches --size 11x13 --niveau 3 --auto-defs --json grille.json
crosswords --size 10x10 --json - | jq '.mots[] | select(.sens == "vertical") | .mot'
```

## Exemple

Une grille de mots fléchés 6x6 de niveau 1 (`--fleches --size 6x6 --niveau 1 --seed 2`) :

```json
{
  "format": "crosswords",
  "version": 1,
  "type": "fleches",
  "largeur": 6,
  "hauteur": 6,
  "graine": 2,
  "niveau": 1,
  "grille": [
    "#C#G#D",
    "PAGE#E",
    "###NON",
    "PAPIER",
    "#CRAIE",
    "REELLE"
  ],
  "mots": [
    {"id": 0, "mot": "PAGE", "sens": "horizontal", "ligne": 1, "colonne": 0, "longueur": 4},
    {"id": 1, "mot": "NON", "sens": "horizontal", "ligne": 2, "colonne": 3, "longueur": 3},
    {"id": 2, "mot": "PAPIER", "sens": "horizontal", "ligne": 3, "colonne": 0, "longueur": 6},
    {"id": 3, "mot": "CRAIE", "sens": "horizontal", "ligne": 4, "colonne": 1, "longueur": 5},
    {"id": 4, "mot": "REELLE", "sens": "horizontal", "ligne": 5, "colonne": 0, "longueur": 6},
    {"id": 5, "mot": "CA", "sens": "vertical", "ligne": 0, "colonne": 1, "longueur": 2},
    {"id": 6, "mot": "ACE", "sens": "vertical", "ligne": 3, "colonne": 1, "longueur": 3},
    {"id": 7, "mot": "PRE", "sens": "vertical", "ligne": 3, "colonne": 2, "longueur": 3},
    {"id": 8, "mot": "GENIAL", "sens": "vertical", "ligne": 0, "colonne": 3, "longueur": 6},
    {"id": 9, "mot": "OEIL", "sens": "vertical", "ligne": 2, "colonne": 4, "longueur": 4},
    {"id": 10, "mot": "DENREE", "sens": "vertical", "ligne": 0, "colonne": 5, "longueur": 6}
  ],
  "definitions": [
    {"numero": 1, "ligne": 0, "colonne": 0, "fleche": "droite-bas", "mot": 5, "texte": "Cela, à l'oral"},
    {"numero": 2, "ligne": 0, "colonne": 0, "fleche": "bas-droite", "mot": 0, "texte": "Feuillet d'un livre"},
    {"numero": 3, "ligne": 0, "colonne": 2, "fleche": "droite-bas", "mot": 8, "texte": "Brillant"},
    {"numero": 4, "ligne": 0, "colonne": 4, "fleche": "droite-bas", "mot": 10, "texte": "Produit alimentaire"},
    {"numero": 5, "ligne": 1, "colonne": 4, "fleche": "bas", "mot": 9, "texte": "Organe de la vue"},
    {"numero": 6, "ligne": 2, "colonne": 0, "fleche": "bas-droite", "mot": 2, "texte": "Feuille à écrire"},
    {"numero": 7, "ligne": 2, "colonne": 1, "fleche": "bas", "mot": 6, "texte": "As de carte"},
    {"numero": 8, "ligne": 2, "colonne": 2, "fleche": "droite", "mot": 1, "texte": "Contraire de oui"},
    {"numero": 9, "ligne": 2, "colonne": 2, "fleche": "bas", "mot": 7, "texte": "Champ d'herbe"},
    {"numero": 10, "ligne": 4, "colonne": 0, "fleche": "droite", "mot": 3, "texte": "Bâton pour écrire"},
    {"numero": 11, "ligne": 4, "colonne": 0, "fleche": "bas-droite", "mot": 4, "texte": "Vraie"}
  ]
}
```

Une grille de mots croisés a la même forme, avec `"type": "croises"`, `"niveau": null` et `"definitions": []`.

## Conventions

- **Coordonnées.** `ligne` et `colonne` partent de 0, en haut à gauche. La rangée affichée « I » est la ligne 0, la colonne affichée « 1 » est la colonne 0.
- **Lettres.** Majuscules A à Z, sans accents ni ligatures (« cœur » s'écrit `COEUR`) : c'est la convention des grilles françaises, et c'est ce que le joueur écrit dans les cases.
- **Champs.** Tous les champs sont toujours présents, éventuellement `null` ou vides, et il n'y en a pas d'autres.

## Champs

### Document

| Champ | Type | Description |
|---|---|---|
| `format` | chaîne | Toujours `"crosswords"`. |
| `version` | entier | Version du format, `1`. Elle change à toute modification incompatible. |
| `type` | chaîne | `"croises"` (mots croisés) ou `"fleches"` (mots fléchés). |
| `largeur`, `hauteur` | entier | Dimensions de la grille, en cases. |
| `graine` | entier ou `null` | Graine qui a produit la grille : même graine, même dictionnaire et mêmes options donnent la même grille. |
| `niveau` | entier ou `null` | Niveau de difficulté des mots fléchés, de 1 (facile) à 4 (très difficile) ; `null` pour les mots croisés. |
| `grille` | tableau de chaînes | Une chaîne par ligne, de haut en bas, de `largeur` caractères chacune. |
| `mots` | tableau | Les mots de la grille, voir ci-dessous. |
| `definitions` | tableau | Les définitions des mots fléchés, voir ci-dessous ; vide pour les mots croisés. |

### `grille`

Chaque caractère est une case :

| Caractère | Case |
|---|---|
| `A` à `Z` | Case blanche et sa lettre dans la solution. |
| `#` | Case noire (mots croisés) ou case définition (mots fléchés). |
| `.` | Case blanche sans lettre (gabarit non rempli ; n'apparaît pas dans une grille générée). |

Pour afficher une grille vierge, remplacer les lettres par des cases vides.

### `mots`

Un mot par emplacement : toute suite maximale d'au moins deux cases blanches, horizontale ou verticale. L'ordre est celui de la lecture : les horizontaux ligne par ligne, puis les verticaux colonne par colonne.

| Champ | Type | Description |
|---|---|---|
| `id` | entier | Position du mot dans le tableau, de 0 à n-1 ; c'est la valeur que reprend `definitions[].mot`. |
| `mot` | chaîne | Le mot, lu dans la grille. |
| `sens` | chaîne | `"horizontal"` (de gauche à droite) ou `"vertical"` (de haut en bas). |
| `ligne`, `colonne` | entier | Case de la première lettre. |
| `longueur` | entier | Nombre de lettres. |

Pour les mots croisés, les définitions « à la française » se regroupent par ligne (« Horizontalement », rangées en chiffres romains) et par colonne (« Verticalement », colonnes en chiffres arabes) : tous les mots horizontaux de même `ligne`, puis tous les verticaux de même `colonne`.

### `definitions` (mots fléchés)

Une entrée par mot, numérotée dans l'ordre de lecture des cases définitions (de gauche à droite, puis de haut en bas) et, dans une case qui en porte deux, de haut en bas. C'est la numérotation de `--numeros`.

| Champ | Type | Description |
|---|---|---|
| `numero` | entier | Numéro de la définition, de 1 à n, sans trou. |
| `ligne`, `colonne` | entier | Case définition qui porte la définition. |
| `fleche` | chaîne | Direction de la flèche, voir ci-dessous. |
| `mot` | entier | `id` du mot défini dans `mots`. |
| `texte` | chaîne ou `null` | La définition ; `null` si elle est inconnue (ni dans le cache de définitions, ni dans `--defs`, et `--auto-defs` absent). |

Une case définition porte une ou deux définitions. Quand elle en porte deux, la case est partagée en deux moitiés : l'entrée de plus petit `numero` occupe celle du haut.

### Flèches

Soit (l, c) la case définition :

| `fleche` | Symbole | Le mot défini |
|---|---|---|
| `droite` | → | horizontal, commence en (l, c+1) |
| `bas` | ↓ | vertical, commence en (l+1, c) |
| `droite-bas` | ↴ | vertical, commence en (l, c+1) : la flèche part vers la droite puis descend (mot partant de la première ligne) |
| `bas-droite` | ↳ | horizontal, commence en (l+1, c) : la flèche descend puis part vers la droite (mot partant de la première colonne) |

## Garanties

Pour un fichier produit par CrossWords :

- `grille` a `hauteur` chaînes de `largeur` caractères ;
- chaque mot de `mots` se relit dans `grille` à partir de (`ligne`, `colonne`) dans son `sens`, sur `longueur` cases, bordé par un bord ou une case `#` à chaque extrémité ;
- toute case blanche appartient à au moins un mot ; aucun mot n'apparaît deux fois ;
- pour les mots fléchés, chaque mot a exactement une définition, chaque case `#` en porte au plus deux, et la position de début du mot se déduit de la case définition et de la flèche comme dans le tableau ci-dessus ;
- pour les mots fléchés générés (sans `--template`), chaque case `#` porte au moins une définition.

## Évolution

Toute modification du format, même l'ajout d'un champ ou d'une valeur, change `version`. Un programme qui lit ce format doit refuser une `version` qu'il ne connaît pas ; le schéma JSON correspond à la version 1.
