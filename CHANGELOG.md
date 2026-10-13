# Journal des modifications

Les versions suivent le [versionnage sémantique](https://semver.org/lang/fr/). La section de chaque version sert de notes de publication sur GitHub.

## [1.1.0] - 2026-10-13

### Ajouts

- Grilles en anglais (`--langue en`), mots croisés comme mots fléchés : dictionnaire an-array-of-english-words noté par la liste collaborative de Crossword Nexus, mêmes seuils et mêmes niveaux qu'en français.
- Présentation anglo-saxonne des mots croisés en anglais : cases numérotées et listes « Across » et « Down », en texte comme en SVG.
- Définitions automatiques en anglais, en orthographe américaine, avec un cache séparé (`definitions-en.txt`).
- Bibliothèque : type `Language`, `wordlist::english`, `autodefs::generate_for` et variantes `_in` des rendus et de l'export JSON ; l'API 1.0 est inchangée.

### Changements

- Format JSON en version 2 : champ `langue`, et `numero` des mots (numérotation anglo-saxonne des cases, pour les mots croisés).

## [1.0.0] - 2026-10-13

Première version publiée.

### Grilles

- Mots croisés à la française : cases noires isolées, sans symétrie imposée, rangées en chiffres romains et colonnes en chiffres arabes ; génération complète ou remplissage d'un gabarit (`--template`).
- Mots fléchés : cases définitions à une ou deux définitions, flèches droites et coudées, définitions dans les cases ou numérotées sous la grille (`--numeros`).
- Quatre niveaux de difficulté pour les mots fléchés (`--niveau 1` à `4`) : vocabulaire, densité de cases définitions et style des définitions.

### Définitions

- Cache de définitions avec niveau, complété par un fichier personnel prioritaire (`--defs`).
- Rédaction automatique des définitions manquantes par l'API Claude (`--auto-defs`) : Claude Haiku 5.5 aux niveaux 1 et 2, Claude Opus 5.5 aux niveaux 3 et 4, avec contrôle de longueur et mise en cache.

### Dictionnaire

- Liste française téléchargée au premier lancement (an-array-of-french-words 2.0.0 et fréquences Lexique 3.83), vérifiée par empreinte SHA-256 et mise en cache ; seuil de fréquence réglable (`--min-score`).

### Sorties

- Texte, SVG imprimable (grille vierge et solution) et JSON (`--json`), format documenté dans `docs/format-json.md` avec un schéma JSON.

### Moteur

- Remplissage par propagation de contraintes, mot le moins contraignant d'abord, budget court par gabarit et nouveaux tirages ; résultat déterminé par la graine.

[1.1.0]: https://github.com/XNinety9/CrossWords/releases/tag/v1.1.0
[1.0.0]: https://github.com/XNinety9/CrossWords/releases/tag/v1.0.0
