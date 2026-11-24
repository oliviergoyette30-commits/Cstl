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
| anthropic/claude-haiku-5.5 | 106/108 | 105/108 | 106/108 | 106/108 |
| anthropic/claude-sonnet-5.5 | 106/108 | 108/108 | 107/108 | 105/108 |
| deepseek/deepseek-v4.1-flash | 36/36 | 36/36 | 36/36 | - |
| gemini-3.5-flash-lite | 23/36 | 23/36 | 19/36 | 24/36 |
| gemma-4-26b-a4b-it | 36/36 | 36/36 | 35/36 | - |
| gemma-4-31b-it | 35/36 | 35/36 | 36/36 | - |
| meta-llama/llama-4-maverick | 74/108 | 89/108 | 77/108 | 89/108 |
| mistralai/mistral-medium-3-5 | 51/108 | 57/108 | 58/108 | 51/108 |
| openai/gpt-5-mini | 108/108 | 106/108 | 107/108 | 107/108 |
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
| Lecture | 610/720 (84.7 %) | 629/720 (87.4 %) | 616/720 (85.6 %) | 482/576 (83.7 %) |
| Ecriture stricte | 110/118 (93.2 %) | 102/116 (87.9 %) | - | 107/120 (89.2 %) |
| Ecriture tolerante | - | - | - | 116/120 (96.7 %) |
| Vide qui parle | 215/240 (89.6 %) | 212/240 (88.3 %) | 214/240 (89.2 %) | - |

JSON naif sur le vide qui parle : 68/240 (28.3 %).

## 4. Donnees reelles hors distribution (PyPI, npm, crates.io, Wikidata)

12 reponses d'API publiques (`scripts/real_data/`, `scripts/real_data_build.py`), questions generees par programme (`scripts/real_data_benchmark.py`), 3 passes en lecture. TOON n'est mesure que sur les 10 documents que `toon_lite` relit a l'identique.

| Modele | JSON | TOON | Arbre indente | Arbre indente + /N |
|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 75/75 | 59/60 | 75/75 | 75/75 |
| deepseek/deepseek-v4.1-flash | 75/75 | 60/60 | 75/75 | 75/75 |
| openai/gpt-5-mini | 75/75 | 60/60 | 74/75 | 73/75 |
| qwen/qwen3.8-flash | 75/75 | 60/60 | 75/75 | 73/75 |
| anthropic/claude-sonnet-5.5 | 75/75 | 60/60 | 75/75 | 75/75 |
| meta-llama/llama-4-maverick | 69/75 | 57/60 | 72/75 | 71/75 |
| mistralai/mistral-medium-3-5 | 75/75 | 60/60 | 75/75 | 75/75 |

**Ecriture** (documents identiques, strict / tolerant) :

| Modele | JSON | TOON | Arbre indente + /N |
|---|---|---|---|
| anthropic/claude-haiku-5.5 | 12/12 / 12/12 | 7/10 / 7/10 | 10/12 / 10/12 |
| anthropic/claude-sonnet-5.5 | 12/12 / 12/12 | 9/10 / 9/10 | 10/12 / 10/12 |
| meta-llama/llama-4-maverick | 10/12 / 10/12 | 4/10 / 4/10 | 3/12 / 3/12 |
| openai/gpt-5-mini | 12/12 / 12/12 | 6/10 / 6/10 | 5/12 / 5/12 |
| mistralai/mistral-medium-3-5 | 8/12 / 8/12 | 3/10 / 3/10 | 7/12 / 7/12 |
| deepseek/deepseek-v4.1-flash | 11/12 / 11/12 | 5/9 / 5/9 | 7/11 / 7/11 |

**Tokens d'entree comptes par chaque fournisseur** (usage.prompt_tokens, sur les memes documents ; « commun » = les documents que TOON relit) :

| Tokenizer | Reel : JSON | Reel : arbre indente | Ecart | Synthetique : JSON | Synthetique : arbre indente | Ecart |
|---|---|---|---|---|---|---|
| openai/gpt-5-mini | 10354 | 9141 | -12 % | 16873 | 9545 | -43 % |
| anthropic/claude-haiku-5.5 | 15557 | 13470 | -13 % | 24775 | 12620 | -49 % |
| meta-llama/llama-4-maverick | 9973 | 9027 | -9 % | 16444 | 9553 | -42 % |
| deepseek/deepseek-v4.1-flash | 11128 | 9815 | -12 % | 17780 | 9474 | -47 % |
| qwen/qwen3.8-flash | 11485 | 10568 | -8 % | 17744 | 10774 | -39 % |
| mistralai/mistral-medium-3-5 | 12728 | 11261 | -12 % | 19115 | 11197 | -41 % |

## 5. Stabilite : 3 passes sur la lecture des donnees imbriquees

Moyenne et ecart-type du score par passe (sur 36), modele par modele.

| Modele | JSON | TOON | Arbre indente | Arbre indente + /N |
|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 35.3 ± 0.6 | 35.0 ± 0.0 | 35.3 ± 0.6 | 35.3 ± 0.6 |
| anthropic/claude-sonnet-5.5 | 35.3 ± 0.6 | 36.0 ± 0.0 | 35.7 ± 0.6 | 35.0 ± 0.0 |
| meta-llama/llama-4-maverick | 24.7 ± 0.6 | 29.7 ± 0.6 | 25.7 ± 1.2 | 29.7 ± 2.5 |
| mistralai/mistral-medium-3-5 | 17.0 ± 1.0 | 19.0 ± 0.0 | 19.3 ± 0.6 | 17.0 ± 0.0 |
| openai/gpt-5-mini | 36.0 ± 0.0 | 35.3 ± 0.6 | 35.7 ± 0.6 | 35.7 ± 0.6 |

## Reproduire

| Element | Ou |
|---|---|
| Grammaire du format | `docs/CSTL_ARBRE_INDENTE_SPEC.md` |
| Encodeur et decodeur de reference | `sdk/python/cstl_compact.py` (`encode_tree_indented`, `decode_tree_indented`) ; tests : `sdk/python/test_cstl_compact.py` |
| Jeux imbriques (12) et questions | `scripts/nested_benchmark.py` : `gen()`, `questions()` (graines fixes) |
| Consignes exactes, notation | meme fichier : `NOTES`, invite dans `main()`, `same()`, `extract_answer()` |
| Ecriture | `scripts/nested_writing_benchmark.py` : `prompt()`, decodeurs, `leaves()` |
| Vide qui parle | `scripts/unknown_benchmark.py` : `gen()`, `questions()`, `NOTES` |
| Donnees reelles | `scripts/real_data/*.json`, `scripts/real_data_build.py`, `scripts/real_data_benchmark.py` |
| Reponses brutes de chaque modele | `scripts/nested_*_<modele>.json`, `scripts/unknown_benchmark.json`, `scripts/real_data_results.json` |
| Parametres | OpenRouter : `temperature=0`, `max_tokens=8000` ; Gemini : parametres par defaut, `maxOutputTokens=16000` (`scripts/team_task_benchmark.py`) |
| Versions des modeles | celles servies par OpenRouter et l'API Gemini aux dates indiquees (2026-11-22 a 2026-11-24), sous les noms du tableau |

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
