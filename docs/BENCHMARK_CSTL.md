# Banc d'essai CSTL : resultats complets et comment participer

Genere par `scripts/multi_model_summary.py --export` a partir des resultats bruts du depot (`scripts/nested_*.json`, `scripts/unknown_benchmark.json`). Aucun chiffre n'est ecrit a la main.

## Ce qui est mesure

Trois tests, memes donnees et memes consignes pour chaque format et chaque modele :

1. **Lecture** : 12 jeux de donnees imbriquees (commandes, organisation, contrats ; 8 et 24 elements ; 2 graines), 3 questions par jeu, soit 36 questions par format. La reponse est le dernier objet `{"r1": ..., "r2": ..., "r3": ...}` du texte.
2. **Ecriture** : le modele convertit les 12 documents JSON dans le format, a partir d'une description et d'un exemple d'une autre famille de donnees. Sa sortie est relue par le vrai decodeur ; on compte les documents identiques au JSON de depart (strict, puis tolerant pour l'arbre indente).
3. **Vide qui parle** : 6 jeux de dossiers patients ou chaque examen est une valeur, null (connu, sans valeur), inconnu (avec son incertitude) ou absent ; 4 questions par jeu, soit 24 par format. Le « JSON naif » ecrit l'inconnu null, comme on le fait d'habitude.

Les jeux en erreur d'API sont exclus du total (x/y), jamais comptes faux.

## Taille (tokens du tokenizer Gemini)

| Format | Donnees imbriquees (12 jeux) | Vide qui parle (6 jeux) |
|---|---|---|
| JSON compact | 17577 (+0 %) | 3697 |
| TOON | 21102 (+20 %) | 4928 |
| CSTL arbre indente | 10792 (-39 %) | 2390 |
| CSTL arbre indente + /N | 11457 (-35 %) | - |

## 1. Lecture (questions justes)

| Modele | JSON | TOON | Arbre indente | Arbre indente + /N |
|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 36/36 | 35/36 | 35/36 | - |
| anthropic/claude-sonnet-5.5 | 35/36 | 36/36 | 36/36 | - |
| deepseek/deepseek-v4.1-flash | 36/36 | 36/36 | 36/36 | - |
| gemini-3.5-flash-lite | 23/36 | 23/36 | 19/36 | - |
| gemma-4-26b-a4b-it | 36/36 | 36/36 | 35/36 | - |
| gemma-4-31b-it | 35/36 | 35/36 | 36/36 | - |
| meta-llama/llama-4-maverick | 25/36 | 30/36 | 25/36 | 32/36 |
| mistralai/mistral-medium-3-5 | 18/36 | 19/36 | 19/36 | 17/36 |
| openai/gpt-5-mini | 36/36 | 35/36 | 36/36 | 35/36 |
| qwen/qwen3.8-flash | 35/36 | 34/36 | 35/36 | - |

## 2. Ecriture (documents identiques au JSON de depart)

| Modele | JSON | TOON | Arbre indente + /N (strict) | Arbre indente + /N (tolerant) |
|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 12/12 | 10/12 | 12/12 | 12/12 |
| anthropic/claude-sonnet-5.5 | 12/12 | 12/12 | 12/12 | 12/12 |
| deepseek/deepseek-v4.1-flash | 10/12 | 8/12 | 10/12 | 10/12 |
| gemini-3.5-flash-lite | 12/12 | 12/12 | 9/12 | 12/12 |
| gemma-4-26b-a4b-it | 10/12 | 11/12 | 12/12 | 12/12 |
| gemma-4-31b-it | 10/10 | 8/10 | 12/12 | 12/12 |
| meta-llama/llama-4-maverick | 11/12 | 8/12 | 8/12 | 12/12 |
| mistralai/mistral-medium-3-5 | 12/12 | 12/12 | 10/12 | 12/12 |
| openai/gpt-5-mini | 12/12 | 12/12 | 12/12 | 12/12 |
| qwen/qwen3.8-flash | 9/12 | 9/10 | 10/12 | 10/12 |

Le mode tolerant (`decode_tree_indented(texte, lenient=True)`) est celui que la documentation recommande pour un texte ecrit par un modele : un compte faux devient un avertissement, des colonnes finales peuvent manquer, un champ facultatif peut etre ecrit `nom=valeur` meme s'il est en colonne. Rien n'est devine : le document relu doit etre identique. JSON et TOON n'ont pas de mode tolerant.

## 3. Vide qui parle (questions justes)

| Modele | JSON naif | JSON | TOON | Arbre indente |
|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 8/24 | 24/24 | 23/24 | 24/24 |
| anthropic/claude-sonnet-5.5 | 8/24 | 24/24 | 24/24 | 24/24 |
| deepseek/deepseek-v4.1-flash | 7/24 | 24/24 | 24/24 | 23/24 |
| gemini-3.5-flash-lite | 6/24 | 16/24 | 16/24 | 18/24 |
| gemma-4-26b-a4b-it | 8/24 | 24/24 | 24/24 | 24/24 |
| gemma-4-31b-it | 8/24 | 24/24 | 24/24 | 24/24 |
| meta-llama/llama-4-maverick | 6/24 | 16/24 | 17/24 | 20/24 |
| mistralai/mistral-medium-3-5 | 1/24 | 15/24 | 13/24 | 9/24 |
| openai/gpt-5-mini | 8/24 | 24/24 | 24/24 | 24/24 |
| qwen/qwen3.8-flash | 8/24 | 24/24 | 23/24 | 24/24 |

## Totaux

| Test | JSON | TOON | Arbre indente | Arbre indente + /N |
|---|---|---|---|---|
| Lecture | 315/360 (87.5 %) | 319/360 (88.6 %) | 312/360 (86.7 %) | 84/108 (77.8 %) |
| Ecriture stricte | 110/118 (93.2 %) | 102/116 (87.9 %) | - | 107/120 (89.2 %) |
| Ecriture tolerante | - | - | - | 116/120 (96.7 %) |
| Vide qui parle | 215/240 (89.6 %) | 212/240 (88.3 %) | 214/240 (89.2 %) | - |

JSON naif sur le vide qui parle : 68/240 (28.3 %).

## Participer : ajouter un modele

**Avec une cle d'API** (Gemini : `gemini-*`, `gemma-*` ; OpenRouter : `fournisseur/modele`, par ex. `openai/gpt-5-mini`) :

```bash
M=openai/gpt-5-mini        # le modele a tester
python3 scripts/nested_benchmark.py --comprehension $M --formats json,toon,cstl_indent,cstl_indent_ligne --resume
python3 scripts/nested_writing_benchmark.py $M --formats json,toon,cstl_indent_ligne --resume
python3 scripts/unknown_benchmark.py --comprehension $M --resume
python3 scripts/multi_model_summary.py --export docs/BENCHMARK_CSTL.md
```

Cles : `GEMINI_API_KEY`, ou `OPENROUTER_API_KEY`. `--resume` ne repose que ce qui manque ou a echoue sur une erreur d'API. Les resultats bruts (toutes les reponses) vont dans `scripts/` ; proposer une PR avec ces fichiers.

**Sans code** (fenetre de discussion, n'importe quel modele) : `scripts/cstl_essai_tout_en_un.py` est un seul fichier Python sans dependance.

```bash
python3 cstl_essai_tout_en_un.py consignes essais/   # 7 consignes a coller, une conversation chacune
# enregistrer chaque premiere reponse dans essais/<test>.reponse.txt
python3 cstl_essai_tout_en_un.py verifier essais/
```

Regles pour un resultat valable : une NOUVELLE conversation par consigne, la PREMIERE reponse telle quelle (sans demander de correction), et le modele ne doit pas voir le script ni le verificateur.

## Limites

- Donnees generees (commandes, organisation, contrats, dossiers patients), pas encore de donnees reelles.
- Une passe par question ; des ecarts de 1 a 3 points sont dans le bruit.
- Les modeles ont vu enormement de JSON a l'entrainement et aucun CSTL : l'arbre indente part avec ce handicap.
