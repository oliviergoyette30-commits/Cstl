# CSTL arbre indenté — spécification v1 (2026-11-24)

Grammaire de référence du format écrit par `encode_tree_indented` et lu par
`decode_tree_indented` (`sdk/python/cstl_compact.py`). L'encodeur et le
décodeur sont la référence : en cas de désaccord avec ce texte, c'est un bug
de ce texte. Vérification : `decode(encode(x)) == x` sur 30 000 documents
JSON aléatoires avec valeurs pièges (`sdk/python/test_cstl_compact.py`).

## 1. Modèle de données

Un document est une valeur JSON quelconque, plus une valeur **inconnue**
(le « vide qui parle »), distincte de `null` et d'un champ absent :

| État | JSON | Arbre indenté |
|---|---|---|
| valeur | `"x"`, `3`, `true` | `x`, `3`, `true` |
| connu, sans valeur | `null` | `null` |
| inconnu | `{"$unknown": {}}` ou `{"$unknown": {"sigma": 0.3}}` | `?` ou `?{sigma:0.3}` |
| sans objet | champ absent | champ non écrit |

## 2. Grammaire (EBNF)

```ebnf
document    = racine , { ligne_entete } , { element } , [ fin ] ;
racine      = [ "@@list" NL | "@@= " valeur_en_ligne NL | "@@ " champs NL { bloc } ] ;
ligne_entete= "@" nom [ "[" entier "]" ] [ "^" parents ] ":" { " " colonne } NL ;
parents     = parent { "," parent } ;          (* "." = la racine *)
element     = indent [ nom ">" " " ] cellules [ " /" entier ] NL { bloc } ;
cellules    = "." | cellule { " " cellule } { " " champ_nomme } | champ_nomme { " " champ_nomme } ;
champ_nomme = nom_champ "=" cellule ;
nom_champ   = /[A-Za-z0-9_.\-]+/ | chaine_json ;
bloc        = indent "| " texte NL | indent "|" NL ;   (* N lignes pour une cellule "|N" *)
fin         = "@end" { " " nom "=" entier } NL ;
indent      = { "  " } ;                          (* 2 espaces par niveau *)
nom         = /[A-Za-z_][A-Za-z0-9_.]*/ ;
```

### Cellules (une valeur dans une ligne)

| Cellule | Valeur |
|---|---|
| `null`, `true`, `false` | null, booléens |
| nombre JSON (`3`, `-1.5`, `2e3`) | nombre ; `0123` est une chaîne |
| mot sans espace | chaîne |
| `"…"` | chaîne (échappements `\"` et `\\`) |
| `[…]`, `{…}` | liste ou objet **en ligne** (section 3) |
| `?`, `?{…}` | inconnu, avec ses détails |
| `\|N` | texte de N lignes, écrites ensuite en bloc `\| …` |

Une chaîne est **entre guillemets** si elle est vide, si elle s'écrit comme
`null`, `true`, `false`, `-`, `.` ou un nombre, si elle contient un espace,
`"`, `#` ou `=`, ou si elle commence par `[`, `{`, `|`, `@`, `/` ou `?`.
Ainsi `"false"`, `"0"`, `""` et `"null"` restent des chaînes.

## 3. Valeurs en ligne

```ebnf
valeur_en_ligne = liste | objet | "?" [ objet ] | chaine_json | mot ;
liste           = "[" [ valeur_en_ligne { "," valeur_en_ligne } ] "]" ;
objet           = "{" [ cle ":" valeur_en_ligne { "," cle ":" valeur_en_ligne } ] "}" ;
cle             = /[A-Za-z0-9_.\-]+/ | chaine_json ;
mot             = /[^\s",\[\]{}:\\]+/ ;              (* typé comme une cellule *)
```

Une chaîne en ligne est en JSON (`"a, b"`) dès qu'elle contient un espace,
`,` `:` `[` `]` `{` `}` `"` ou `\`, ou qu'elle ressemble à un nombre, à
`null`/`true`/`false`, ou commence par `?`.

## 4. Structure

- **Une liste d'objets** devient une table : `@nom: col1 col2`, déclarée une
  fois. Une **colonne** est un champ présent dans **tous** les éléments de la
  liste. Un champ présent dans certains seulement s'écrit `nom=valeur` en fin
  de ligne, et seulement là où il existe.
- **Enfants** : une liste d'objets à l'intérieur d'un élément est une table
  `@enfant^parent:`. Ses éléments sont écrits sous leur parent, indentés de
  2 espaces de plus.
- **Plusieurs listes au même niveau** : chaque ligne commence par `nom> `.
- Un élément sans champ s'écrit `.`.
- Tout le reste (objet qui n'est pas une liste d'objets, liste de valeurs,
  liste vide) est une valeur en ligne.

## 5. Comptes

- `[N]` après le nom (`@orders[8]:`) : N éléments en tout. **Facultatif** ;
  vérifié s'il est présent.
- `/N` en fin de ligne : nombre d'**enfants directs** de l'élément.
  Recommandé quand un modèle écrit (`counts="line"`).
- `@end` : fin du document ; avec `nom=N`, les comptes totaux.

## 6. Lecture stricte et lecture tolérante

**Strict** (défaut, entre machines) : toute règle violée est une erreur
(`CompactError`), avec le numéro de ligne.

**Tolérant** (`lenient=True`, pour un texte écrit par un modèle) assouplit
exactement trois règles. Rien d'autre n'est accepté, et rien n'est deviné :

| Règle assouplie | Effet | Signalé |
|---|---|---|
| comptes `[N]`, `/N`, `@end` faux | lu quand même | avertissement |
| dernières colonnes absentes d'une ligne | ces champs sont absents | avertissement, ligne nommée |
| largeur d'indentation (2, 4 espaces, tabulation) | acceptée si cohérente | — |

Toujours refusé, même en tolérant : indentation incohérente, en-tête
illisible, ligne sans liste d'enfants déclarée, guillemets non fermés,
valeur en ligne mal formée, bloc `|N` incomplet, contenu après `@end`.

Les bancs d'essai publient **les deux scores, séparés** : strict et tolérant.

## 7. Exemple

```
@@ magasin=Montreal
@commandes: id client date
@lignes^commandes: article qte prix
c1 Alice 2026-11-20 /2
  clavier 2 45
  souris 1 25 garantie={duree:2}
c2 Bob 2026-11-21 note="livrer avant 17h" paiement=?{raison:banque} /1
  ecran 1 220
@end
```
