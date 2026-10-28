//! CSTL v5.0.0 — Validateur sémantique — VERSION FINALE
//!
//! Confirmée contre le code source réel lu sur Termux (ast.rs, parser.rs) :
//! - Relation { subject, operator, object, attrs: Vec<Field>, modality: Option<String>, line }
//! - Field { name, type_hint, value, line }
//! - CstlDocument.relations: Vec<Relation> contient À LA FOIS les lignes de
//!   RELATIONS et de CONSTRAINTS (parser.rs ligne 355 : même chemin de code,
//!   is_modality() détermine juste si `modality` est rempli ou non).
//! - AUCUN besoin d'extracteur de triplets : self.relations.push(rel) existe
//!   déjà (parser.rs lignes 397 et 530), correctement décomposé.
//! - LE VRAI TROU : aucun contrôle n'existe qui compare `operator` contre
//!   la liste des 36 opérateurs officiels, ni qui vérifie l'axiome D de SDL.
//!   C'est uniquement ce que ce module ajoute.
//!
//! Session du 9 juillet 2026 : ajout du support domaine (with_domain), qui
//! délègue à crate::domains::is_domain_operator pour accepter les verbes
//! d'un domaine (ex. PRESCRIBE en médical, anglais depuis le 2026-10-05) en
//! plus des opérateurs du noyau.

use crate::ast::Relation;

/// Opérateurs dépréciés depuis v5.0 : encore reconnus (warning, pas erreur),
/// migration recommandée. Vivait avant dans validator_semantic.rs (module
/// supprimé le 2026-09-04, item #2 de la liste des choses a faire -- son
/// seul autre contenu etait un systeme de validation Block/AST jamais
/// branche sur le chemin TCP reel, voir ast.rs pour le detail) -- rapatrie
/// ici, seul consommateur reel restant.
pub const DEPRECATED_OPERATORS: &[&str] = &["MUTUAL"];

/// `pub(crate)` depuis 2026-09-29 (Couche 5, compression structurelle) :
/// `compression::structural` importe cette liste directement pour sa table
/// d'opcodes plutot que de la dupliquer -- meme raison que
/// `parser::VALID_CONSTRAINT_MODALITIES`/`VALID_UNCERTAINTY_STATUSES`.
pub(crate) const OFFICIAL_OPERATORS: &[&str] = &[
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST", "AMP", "INH",
    "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
    "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES",
    "COMPARES", "ENTAILS", "CONTRADICTS",
    "KNOWS", "BELIEVES", "ASSUMES", "DOUBTS", "DISBELIEVES",
    "BEFORE", "AFTER", "DURING",
    // AJOUTE EN DERNIER, JAMAIS INSERE AU MILIEU : structural.rs encode cet
    // operateur par SA POSITION dans ce tableau (un octet, voir
    // `OFFICIAL_OPERATORS.iter().position(...)` / `.get(op_byte as usize)`
    // dans compression/structural.rs) -- inserer au milieu decalerait tous
    // les index suivants et casserait le decodage de tout payload compresse
    // avec une version anterieure de cette liste. Appender est la seule
    // insertion sure.
    "EITHER_OR",
    "REACTS",
    "INVOLVES",
    "SATISFIES",
];

/// GARDE-FOU 2026-10-28 (session en cours, "On fix tout") -- reponse directe
/// a la trouvaille de ce soir : `REACTS`, `INVOLVES`, `SATISFIES` sont entres
/// dans `OFFICIAL_OPERATORS` au fil de sessions de diagnostic kappa, sans
/// qu'aucune revue independante ne tranche "cette relation merite-t-elle un
/// NOUVEL operateur, ou devrait-elle rentrer dans une case existante ?" --
/// la meme session qui rencontrait le trou decidait aussi de le combler.
/// Resultat mesure ce soir : 25 des 40 operateurs officiels actuels n'ont
/// AUCUNE regle de validation semantique dediee nulle part dans ce fichier
/// (compte exact, voir `test_every_official_operator_is_classified`
/// ci-dessous) -- pire que le ratio 22/36 de l'audit du 26 septembre, et
/// exactement dans le sens attendu : le catalogue grossit plus vite que sa
/// couverture formelle.
///
/// Ce module ne peut pas, a lui seul, forcer une revue HUMAINE independante
/// avant chaque ajout (ca, c'est une regle de branche GitHub -- hors de la
/// portee d'un commit de code, a activer par le proprietaire du depot dans
/// Settings > Branches > Protection rules, idealement avec un fichier
/// CODEOWNERS sur ce fichier). Ce qu'il PEUT forcer, mecaniquement, via
/// `cargo test` (donc CI) : qu'un nouvel operateur ajoute a
/// `OFFICIAL_OPERATORS` ne puisse plus jamais entrer SANS CHOIX EXPLICITE et
/// VISIBLE DANS LE DIFF -- soit il gagne une vraie regle de validation
/// (`check_xxx` dedie, et son nom rejoint `OPERATORS_WITH_FORMAL_SEMANTICS`
/// ci-dessous), soit il est ajoute, a decouvert, a
/// `GRANDFATHERED_UNDEFINED_OPERATORS` avec une justification en
/// commentaire. Un operateur absent des DEUX listes fait echouer
/// `cargo test` -- c'est le garde-fou : pas plus de dette semantique
/// SILENCIEUSE, meme si la dette elle-meme (les 25 operateurs ci-dessous)
/// n'est pas resorbee ce soir.
///
/// Operateurs ayant une regle de validation semantique dediee (au-dela du
/// controle structurel generique Axiome D/K qui s'applique a N'IMPORTE quel
/// operateur portant une modalite MUST/MUST_NOT, donc ne compte pas comme
/// "semantique a CET operateur") :
///   - ENTAILS            -> check_axiom_k_entailment, check_entails_transitivity
///   - CONTRADICTS         -> check_contradicts_symmetry
///   - KNOWS               -> check_knows_calibration
///   - DOUBTS              -> check_doubts_calibration
///   - BELIEVES/DISBELIEVES -> check_epistemic_antonym_consistency
///   - BEFORE/AFTER/DURING -> check_temporal_pair_consistency (DURING ajoute
///     2026-10-28 : exclusion structurelle pure avec BEFORE/AFTER, Allen
///     interval algebra, pas un motif d'usage mesure)
///   - MAINTAIN            -> check_maintain_tau
///   - AMP/INH             -> check_amp_inh_conflict
///   - PERFORM/ARR/ARR.CREATE/ARR.PRODUCE/ARR.JOIN/ARR.ACCESS ->
///     check_temporal_contradiction (via `PERFORMED_OPERATORS` ; ARR.JOIN/
///     ARR.ACCESS ajoutes 2026-10-28, meme famille "Causalite" que les 3
///     autres au spec CSTL_SPEC_v5_0.md:378, oubli corrige, pas une
///     distinction nouvelle)
///   - RESEMBLES/CO_LOCATES/OPPOSES -> check_symmetric_operator_redundancy
///     (2026-10-28 : symetrie par definition du predicat ET confirmee
///     explicitement par CSTL_SPEC_v5_0.md:396 pour OPPOSES, meme caractere
///     que W602/CONTRADICTS)
///   - EQUALS/OPPOSES      -> aussi check_equals_vs_opposition,
///     check_equals_transitivity (2026-10-28 : identite totale vs exclusion
///     mutuelle sur la meme paire est une impossibilite logique pure, pas
///     une regle d'usage)
///   - REACTS              -> check_reacts_valence (2026-10-28 : applique
///     l'enumeration valence= que l'operateur s'est deja donnee dans sa
///     propre doc d'ajout, jamais verifiee avant)
///   - INVOLVES            -> check_involves_role (2026-10-28 : meme
///     logique pour role=, optionnel mais verifie quand present)
///   - EITHER_OR           -> check_either_or_wellformed (2026-10-28 :
///     sujet=objet est une malformation structurelle, pas une regle
///     d'usage)
///   - ASSUMES             -> check_assumes_calibration (2026-10-28 :
///     applique le contrat sigma deja promis par CSTL_SPEC_v5_0.md §10.4
///     (0.30-0.75), meme famille que KNOWS/DOUBTS deja câbles ; distinct de
///     l'antonyme manquant, volontairement laisse de cote, voir
///     GRANDFATHERED_UNDEFINED_OPERATORS ci-dessous)
// `allow(dead_code)` : consommee uniquement par le garde-fou
// `test_every_official_operator_is_classified` (#[cfg(test)]), donc invisible
// a l'analyse dead_code d'un `cargo build` normal -- ce n'est pas du code
// mort par accident, retirer ce const casserait le garde-fou en silence.
#[allow(dead_code)]
pub(crate) const OPERATORS_WITH_FORMAL_SEMANTICS: &[&str] = &[
    "ARR", "ARR.CREATE", "ARR.PRODUCE", "ARR.JOIN", "ARR.ACCESS", "PERFORM",
    "MAINTAIN", "AMP", "INH",
    "ENTAILS", "CONTRADICTS", "OPPOSES", "EQUALS",
    "KNOWS", "BELIEVES", "DISBELIEVES", "DOUBTS",
    "BEFORE", "AFTER", "DURING",
    "RESEMBLES", "CO_LOCATES",
    "REACTS", "INVOLVES",
    "EITHER_OR",
    "ASSUMES",
];

/// Dette semantique EXISTANTE, reconnue a decouvert plutot que silencieuse.
/// Chaque entree ici est un operateur officiel SANS regle de validation
/// dediee -- cette liste ne doit JAMAIS grossir en silence (voir le test de
/// garde-fou) : un nouvel operateur ajoute a `OFFICIAL_OPERATORS` qui
/// atterrit ici doit porter sa propre justification en commentaire au
/// moment de l'ajout, visible dans la revue du diff/PR. Elle PEUT retrecir
/// -- c'est le but -- a mesure qu'un `check_xxx` dedie est ecrit pour l'un
/// de ces operateurs et que son nom migre vers
/// `OPERATORS_WITH_FORMAL_SEMANTICS` ci-dessus.
///
/// 2026-10-28 : retrecie de 25 a 15 ce soir (voir les 10 operateurs listes
/// ci-dessus avec leur date d'ajout). Les 15 restants ne sont PAS laisses
/// ici par manque de temps -- chacun a ete considere et ecarte pour une
/// raison precise :
///   - COMMAND/ASK/STATE/RECOMMEND : actes de parole (FIPA-ACL). Une regle
///     du genre "COMMAND(x) + FORBID(x) par le meme emetteur = contradiction"
///     exigerait d'inventer une inference COMMAND => MUST implicite qui
///     n'existe nulle part ailleurs dans cette grammaire -- de la semantique
///     speculative, pas une application d'une regle deja posee.
///   - TRANSMIT_FAITHFUL/TRANSMIT_INFER : paire candidate a un antonyme
///     (meme patron qu'EPISTEMIC_ANTONYMS), mais AUCUN cas d'echec mesure
///     ne la justifie (contrairement a BELIEVES/DISBELIEVES, trouvaille
///     empirique documentee a `EPISTEMIC_ANTONYMS` ci-dessous). Meme
///     discipline que ce commentaire appliquait deja a KNOWS/ASSUMES pour
///     l'ANTONYME : "pas oublie, juste non mesure" -- note bien, ceci ne
///     concerne QUE l'antonyme manquant, pas toute semantique pour cet
///     operateur ; ASSUMES lui-meme vient d'en gagner une (calibration
///     sigma, voir ci-dessus) sur un fondement totalement different
///     (contrat sigma deja promis par CSTL_SPEC_v5_0.md §10.4, pas un
///     antonyme speculatif).
///   - POSSESSES : usage double documente (attribut/valeur vs role
///     thematique, voir THEMATIC_ARGUMENT_OPERATOR) -- aucune detection
///     automatique ne distingue les deux sens, donc aucune regle fiable
///     n'est postulable sans la construire d'abord (deja note comme
///     "travail separe" a l'endroit meme ou POSSESSES est documente).
///   - COMPARES : aucune contrainte logique evidente au-dela du typage de
///     base ; inventer une regle ici serait pure decoration.
///   - SATISFIES : couple naturellement a CONSTRAINTS (REQUIRE/UNLESS),
///     mais cable cette interaction correctement est un vrai chantier, pas
///     une regle d'une ligne -- laisse pour une session dediee.
///   - INTENT, TRANSFORM, RESIST, PRESSURE, CATALYZE : aucun antonyme ou
///     contrainte structurelle evidente trouvee ce soir ; pourraient en
///     avoir une, pas identifiee avec assez de confiance pour l'ecrire
///     sans tomber dans la meme ambiguite deguisee que ce garde-fou existe
///     pour prevenir.
#[allow(dead_code)]
pub(crate) const GRANDFATHERED_UNDEFINED_OPERATORS: &[&str] = &[
    "INTENT", "TRANSFORM", "RESIST",
    "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "RECOMMEND",
    "POSSESSES", "COMPARES",
    "SATISFIES",
];

/// `EITHER_OR` (2026-10-05) -- trouvaille du run restreint du pipeline kappa
/// v2 (`kappa_generate_v2_deterministic_literals.py`, item complex_002,
/// "Hypertension >160 mmHg requires ACE inhibitors OR calcium channel
/// blockers"). Avant cet ajout, OFFICIAL_OPERATORS n'avait AUCUN operateur
/// de disjonction pour RELATIONS -- un encodeur qui voit "A requires X or Y"
/// n'avait d'autre choix que deux relations REQUIRES paralleles, que l'etape
/// de reconstruction (et tout lecteur humain) lit par defaut comme une
/// CONJONCTION ("requires both X and Y"). Mesure concretement : "ACE
/// inhibitors or calcium channel blockers" (un des deux suffit) reconstruit
/// en "requires both ACE inhibitors and calcium channel blockers" (les deux
/// obligatoires) -- dans un contexte clinique, une instruction materiellement
/// differente, pas cosmetique.
///
/// Usage attendu : `(objet_A) EITHER_OR (objet_B) [id=rNNN]`, une relation
/// SEPAREE qui declare deux objets d'un meme REQUIRES (ou autre operateur)
/// comme alternatives mutuellement satisfaisantes, suivant le meme patron
/// que `ENTAILS`/`CONTRADICTS` (relation logique entre deux propositions,
/// pas entre deux RELATIONS entieres -- voir §10.3 du spec).
///
/// Honnetete sur la portee, meme pattern que `UNCERTAINTY` (§8) et
/// `EPISTEMIC_ANTONYMS` au-dessus : cet ajout fait passer `EITHER_OR` la
/// whitelist (E101) et le rend disponible aux encodeurs/prompts qui
/// referencent `OFFICIAL_OPERATORS`. AUCUNE verification semantique
/// n'exploite encore cette disjonction -- rien ne detecte aujourd'hui qu'un
/// payload avec deux REQUIRES paralleles AURAIT DU etre relie par
/// `EITHER_OR`, et rien ne garantit qu'un encodeur l'utilise spontanement
/// sans qu'on le lui demande explicitement dans le prompt (c'est exactement
/// la lecon du meme run : un prompt qui ne mentionne pas un operateur ne le
/// fait pas apparaitre). Cable cette exploitation reste un travail separe,
/// non fait ici.
pub const DISJUNCTION_OPERATOR: &str = "EITHER_OR";

/// `REACTS` (2026-10-05) -- trouvaille de la comparaison haiku/sonnet sur
/// les 5 items d'echec du run kappa v2 (voir
/// claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md dans le projet), item
/// medium_003 : "Employees were upset about decision" (CEO announced
/// layoffs). Avant cet ajout, OFFICIAL_OPERATORS n'avait AUCUN operateur
/// de stance affective (un sujet oriente emotionnellement vers un objet,
/// sans le produire ni le causer) -- les deux modeles testes ont improvise
/// des contournements faux : haiku a hallucine un contenu epistemique
/// absent de l'original ("employees believe the decision"), sonnet a
/// invente `POSSESSES(Employees,upset) + CATALYZE(upset,decision)`, ce qui
/// INVERSE la causalite (laisse croire que l'etat "upset" PRODUIT une
/// nouvelle decision, alors que l'original dit l'inverse : les employes
/// sont contraries PAR une decision deja prise). Deux modeles, deux erreurs
/// differentes sur le meme trou structurel -- signal que ce n'est pas une
/// faiblesse d'un modele en particulier.
///
/// Usage attendu : `(subject) REACTS (object) [id=rNNN, valence=negative|
/// positive|neutral|mixed, affect=<libre>]`. `valence` porte l'information
/// structurellement utile (polarite, exploitable en aval sans dependre du
/// mot exact) ; `affect` est un descriptif libre optionnel pour la
/// precision (ex: "upset", "angry", "delighted"), suivant exactement le
/// meme patron que `manner=` (§13.2 du spec) -- aucune validation de ces
/// attributs n'est faite ici, `take_trailing_attrs` (parser.rs) est deja
/// generique et les accepte sans modification.
///
/// Choix delibere d'UN SEUL operateur generique plutot qu'une famille
/// (UPSET/ANGRY/PLEASED/...) : contrairement a la famille epistemique (5
/// verbes distincts, structurellement necessaires pour la detection de
/// contradiction via `EPISTEMIC_ANTONYMS`), l'emotion est un ensemble
/// ouvert sans valeur ajoutee mesuree pour le raisonnement deontique de
/// CSTL au-dela de la polarite et de la cible -- exploser le catalogue en
/// dizaines d'operateurs d'emotion violerait l'economie deja etablie par
/// `value=`/`manner=`, et chaque entree supplementaire dans
/// OFFICIAL_OPERATORS coute un octet d'encodage positionnel irreversible
/// (voir le commentaire d'EITHER_OR ci-dessus).
///
/// Honnetete sur la portee, meme pattern que `EITHER_OR` et
/// `EPISTEMIC_ANTONYMS` : cet ajout fait passer `REACTS` la whitelist
/// (E101) et le rend disponible aux encodeurs/prompts. AUCUNE verification
/// semantique n'exploite encore `valence=`/`affect=` -- pas de detection de
/// contradiction affective, pas de propagation dans ExecutionLab. Cable
/// cette exploitation reste un travail separe, non fait ici.
pub const AFFECTIVE_STANCE_OPERATOR: &str = "REACTS";

/// `INVOLVES` (2026-10-05) -- trouvaille de l'audit des 4 items de
/// desaccord du kappa final a 3 juges externes
/// (claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md), confirmee par grep sur
/// tout le corpus existant. `POSSESSES` etait utilise pour DEUX sens
/// distincts : (a) ascription attribut/valeur, le sens fondateur documente
/// dans PRINCIPES.md ("le vide qui parle" -- renal_function POSSESSES
/// value [UNKNOWN=true, ...]), et (b) le role thematique d'un evenement
/// envers son argument (patient/theme/duree), ex. mesure concretement :
/// `(signing) POSSESSES (contract)` [medium_005], `(took) POSSESSES
/// (antibiotics/week)` [medium_002], `(hiring) POSSESSES (Sophie)`
/// [edge_004], `(implementation) POSSESSES (measures)` [complex_001]. Le
/// sens (b) n'est pas de la possession -- un evenement de signature ne
/// "possede" pas un contrat, il le porte comme argument -- et produit des
/// reconstructions bancales ("which entails an agreement" pour
/// medium_005) qui ont contribue aux desaccords de juges sur ce run.
/// `POSSESSES` reste accepte pour l'usage (b) par retrocompatibilite
/// (meme discipline que MUTUAL en son temps, §10.2 du spec) mais la
/// migration recommandee est `INVOLVES`.
///
/// Usage attendu : `(event) INVOLVES (argument) [id=rNNN, role=theme|
/// patient|duration|instrument]`. `role` est optionnel, suit le meme
/// patron que `valence=`/`manner=` -- aucune validation supplementaire,
/// `take_trailing_attrs` (parser.rs) l'accepte sans modification.
///
/// Honnetete sur la portee, meme pattern que EITHER_OR/REACTS ci-dessus :
/// cet ajout fait passer INVOLVES la whitelist (E101). AUCUNE detection
/// automatique ne distingue encore un usage (a) legitime de POSSESSES d'un
/// usage (b) qui AURAIT DU etre INVOLVES -- rien ne force la migration,
/// rien ne l'empeche. Cable cette detection reste un travail separe.
pub const THEMATIC_ARGUMENT_OPERATOR: &str = "INVOLVES";

/// `SATISFIES` (2026-10-05) -- meme audit que INVOLVES ci-dessus. Troisieme
/// sens trouve sous `POSSESSES` : la satisfaction d'une precondition ou
/// d'une autorisation, mesure concretement sur `complex_004` --
/// `(disclosure) POSSESSES (consent)` reconstruit en "disclosure is
/// forbidden unless **it possesses** prior_written consent", une
/// formulation de surface ambigue (sujet grammatical de "possesses" peu
/// clair) qui a directement contribue au desaccord d'un des trois juges
/// sur cet item. "La divulgation possede le consentement" n'est pas une
/// relation de possession -- c'est "la condition est remplie". `POSSESSES`
/// reste accepte pour cet usage par retrocompatibilite ; migration
/// recommandee vers `SATISFIES`.
///
/// Usage attendu : `(subject) SATISFIES (condition) [id=rNNN]`, typiquement
/// a l'interieur d'une clause `(UNLESS)` ou `(IF)` de CONSTRAINTS -- ex.
/// `(UNLESS) disclosure SATISFIES consent [id=c2]` au lieu de `(UNLESS)
/// disclosure POSSESSES consent [id=c2]`.
///
/// Honnetete sur la portee, meme pattern que ci-dessus : passe la
/// whitelist (E101) uniquement. Aucune verification semantique cablee --
/// pas de detection qu'une CONSTRAINTS (UNLESS)/(IF) AURAIT DU utiliser
/// SATISFIES plutot que POSSESSES, pas de propagation dans ExecutionLab.
pub const CONDITION_SATISFACTION_OPERATOR: &str = "SATISFIES";

/// Antonyme officiel de `BELIEVES` (2026-09-23) -- trouvaille du test de
/// comprehension AI-to-AI (`cstl_comprehension_test.py`, item edge_001,
/// "Alice does not believe Bob left early"): avant ce fix, aucun antonyme
/// n'existait pour BELIEVES/KNOWS/ASSUMES dans OFFICIAL_OPERATORS, donc
/// l'encodeur a improvise "DISBELIEVES" -- un mot qui PASSAIT quand meme
/// (check_operator_whitelist n'est qu'un avertissement, jamais bloquant),
/// mais que ni ce module ni ExecutionLab ne reconnaissaient comme lie a
/// BELIEVES. Resultat mesure : un agent qui repond a "le texte affirme-t-il,
/// comme fait independant, que Bob N'EST PAS parti tot" a repondu "no" (une
/// hallucination) au lieu de "unknown" -- conflation croyance/fait que la
/// verification de coherence deterministe aurait du pouvoir signaler mais
/// ne pouvait pas, faute de connaitre l'antonyme. Portee volontairement
/// minimale : seul BELIEVES a un antonyme officiel pour l'instant (c'est le
/// seul cas mesure empiriquement) -- KNOWS et ASSUMES n'en ont pas encore,
/// pas oublie, juste non mesure.
/// `pub` depuis le 2026-09-23 (extension a l'historique, voir
/// `execution_lab.rs::check_epistemic_consistency_with_history`) --
/// reutilisee telle quelle plutot que dupliquee, pour que les deux checks
/// (intra-payload E703 ici, et l'equivalent cross-historique la-bas) ne
/// puissent jamais diverger sur la liste des antonymes reconnus.
pub const EPISTEMIC_ANTONYMS: &[(&str, &str)] = &[("BELIEVES", "DISBELIEVES")];

/// Performatifs FIPA-ACL (Foundation for Intelligent Physical Agents,
/// Agent Communication Language) -- ajoutes le 2026-09-06, en reponse a une
/// lacune reelle trouvee en verifiant le code: `INTENT_PAYLOAD [purpose=...]`
/// existe depuis le debut comme champ d'ENVELOPPE (le type d'acte de
/// communication), separe des `RELATION [type=...]` qui portent le CONTENU
/// (COMMAND/ASK/STATE/PERFORM/RECOMMEND ci-dessus inclus) -- mais `purpose`
/// n'a jamais ete qu'une chaine libre non structuree pour la communication
/// ordinaire entre agents (seuls `agent_register`/`council_decision`/
/// `detect_emergence` recoivent un traitement special dans handler.rs, tous
/// des purposes de CONTROLE de protocole, pas des actes de communication
/// agent-a-agent). Cette liste ferme cet ecart avec un vocabulaire reel,
/// deja standardise dans la litterature multi-agents (Austin/Searle pour la
/// theorie des actes de langage, FIPA pour la liste concrete), plutot que
/// d'inventer une taxonomie ad hoc.
///
/// Volontairement PUREMENT ADDITIF: un payload dont `purpose` n'apparait pas
/// ici continue de fonctionner exactement comme avant (aucune validation
/// stricte de `purpose` n'existe ni n'est ajoutee ici) -- seul le fait de
/// RECONNAITRE un performatif FIPA change quelque chose (voir
/// `server/handler.rs`, bloc `PERFORMATIVE` ajoute a la reponse). Les
/// operateurs d'actes de langage deja existants (COMMAND/ASK/STATE/PERFORM/
/// RECOMMEND, ligne 33) restent inchanges et coexistent sans conflit: ils
/// vivent au niveau RELATION (le contenu de l'acte), le performatif FIPA vit
/// au niveau INTENT_PAYLOAD (le type d'acte lui-meme) -- les deux couches
/// deja separees par le format wire depuis le debut du projet.
pub const FIPA_PERFORMATIVES: &[&str] = &[
    "REQUEST", "INFORM", "QUERY_IF", "QUERY_REF",
    "PROPOSE", "ACCEPT_PROPOSAL", "REJECT_PROPOSAL", "CFP",
    "AGREE", "REFUSE", "CANCEL",
    "CONFIRM", "DISCONFIRM", "FAILURE", "NOT_UNDERSTOOD",
    "SUBSCRIBE",
];

/// Vrai si `purpose` correspond (insensible a la casse) a un performatif
/// FIPA reconnu -- utilise par `server/handler.rs` pour ajouter un bloc
/// `PERFORMATIVE` informatif a la reponse, jamais pour rejeter quoi que ce
/// soit (voir commentaire de `FIPA_PERFORMATIVES` ci-dessus).
pub fn is_fipa_performative(purpose: &str) -> bool {
    let upper = purpose.to_ascii_uppercase();
    FIPA_PERFORMATIVES.contains(&upper.as_str())
}

/// Negociation FIPA minimale (2026-09-06) -- ajoutee en reponse a une
/// lacune reelle constatee APRES `FIPA_PERFORMATIVES` ci-dessus: le
/// mecanisme etait "dormant", le serveur RECONNAISSAIT un performatif
/// (bloc `PERFORMATIVE`) mais ne reagissait jamais differemment selon son
/// type -- un `PROPOSE` et un `REFUSE` recevaient le meme traitement de
/// fond, aucune boucle PROPOSE -> REFUSE n'etait fermee. Choix de portee,
/// assumes et volontairement PETITS (voir aussi `server/handler.rs`, bloc
/// `NEGOTIATION`, et README.md section Layer 7) :
///
/// 1. PAS de moteur de negociation automatique: le serveur ne GENERE
///    jamais de contre-proposition lui-meme -- ca reste le role de
///    l'agent/LLM client, qui a besoin du contexte exact de ce qui a ete
///    refuse pour la construire. Ce module se contente de retrouver et
///    d'exposer ce contexte de facon verifiable (contre l'ADN store reel,
///    pas une supposition).
/// 2. Seuls les performatifs qui CLOTURENT une proposition anterieure
///    recoivent un statut ici: `REFUSE`/`REJECT_PROPOSAL` (statut
///    "refused", la boucle qui manquait completement) et
///    `ACCEPT_PROPOSAL` (statut "accepted", ajoute par symetrie -- meme
///    besoin de retrouver QUELLE proposition a ete acceptee). `PROPOSE`
///    et `CFP` n'ouvrent rien ici (ce sont eux la proposition), `AGREE`
///    est un accord sur une ACTION demandee (semantique FIPA distincte
///    d'ACCEPT_PROPOSAL) et reste hors de ce mecanisme minimal.
/// 3. La correlation utilise un nouveau champ de wire format additif,
///    `INTENT_PAYLOAD.in_reply_to=<hash>` (le hash `sha256:...` retourne
///    dans le bloc `AUDIT` de la reponse au `PROPOSE`/`CFP` original) --
///    PAS la colonne `conversation_id` deja presente dans le schema
///    `adn_store` (jamais peuplee depuis aucun payload, voir
///    `server/handler.rs` ou `put()` la recoit toujours a `None`):
///    `conversation_id` designerait un FIL entier, alors qu'une reponse
///    FIPA refere toujours a UN message precis -- overloader le champ
///    existant aurait ete plus simple a court terme mais aurait cache
///    cette distinction. `in_reply_to` est un champ d'ENVELOPPE comme
///    `purpose`/`sender`/`receiver`, donc deja lisible sans aucun
///    changement de parser (`payload.intent` est une HashMap generique).
pub fn negotiation_status_for(purpose: &str) -> Option<&'static str> {
    match purpose.to_ascii_uppercase().as_str() {
        "REFUSE" | "REJECT_PROPOSAL" => Some("refused"),
        "ACCEPT_PROPOSAL" => Some("accepted"),
        _ => None,
    }
}

// `pub` depuis le 2026-09-04: reutilisees par
// execution_lab::check_deontic_consistency_with_history (Couche 8, audit
// deontique HISTORIQUE) pour rester l'unique source de verite sur ce qui
// compte comme "obligatoire" vs "interdit" -- plutot que dupliquer ces deux
// listes et risquer qu'elles divergent de l'Axiome D intra-payload
// (check_axiom_d ci-dessous).
pub const FORBIDDEN_MODALITIES: &[&str] = &["MUST_NOT", "FORBID"];
pub const REQUIRED_MODALITIES:  &[&str] = &["MUST", "REQUIRE"];
// FIX 2026-10-28 : ARR.JOIN et ARR.ACCESS manquaient ici alors que
// CSTL_SPEC_v5_0.md ligne 378 les classe explicitement dans la MEME
// categorie "Causalite" que ARR/ARR.CREATE/ARR.PRODUCE (les 5 ARR.*
// representent tous un agent qui PERFORME une action causale -- creer,
// rejoindre, produire, acceder). Ce n'etait pas une distinction semantique
// deliberee, juste un oubli : 2 des 5 operateurs de la meme famille
// n'avaient jamais ete branches sur E108 (contradiction tau-locale entre un
// FORBID/MUST_NOT et l'action reellement performee au meme instant), donc
// "(monitor) MUST_NOT ARR.ACCESS patient_record" suivi de
// "(monitor) ARR.ACCESS patient_record" au meme tau passait sans
// detection -- exactement le genre de contradiction que ce check existe
// pour attraper pour les 3 autres membres de la meme famille.
const PERFORMED_OPERATORS:  &[&str] = &["PERFORM", "ARR", "ARR.CREATE", "ARR.PRODUCE", "ARR.JOIN", "ARR.ACCESS"];
const VALID_TAU: &[&str] = &["p", "n", "f", "p_past", "n_present", "f_future"];

/// R9 (port depuis parser.py ATTRIBUTE_ONTOLOGY) — valeurs canoniques pour
/// les 8 clés d'attributs sémantiques. Hors ontologie = warning, pas erreur.
const ATTRIBUTE_ONTOLOGY: &[(&str, &[&str])] = &[
    ("polarity", &["positive", "negative", "neutral"]),
    ("quantifier", &["universal", "existential", "negative", "partial", "plural", "singular", "definite", "indefinite"]),
    ("frequency", &["always", "often", "sometimes", "rarely", "never", "occasional", "habitual", "exclusive"]),
    ("scope", &["universal", "partial", "wide", "narrow", "distributive", "collective", "reflexive", "external"]),
    ("mood", &["indicative", "imperative", "interrogative", "subjunctive", "conditional", "optative"]),
    ("aspect", &["perfective", "imperfective", "progressive", "habitual", "iterative", "perfect"]),
    ("epistemic", &["known", "unknown", "estimated", "inferred", "believed", "doubted", "certain"]),
    ("evidential", &["visual", "hearsay", "inference", "direct", "report"]),
];

fn ontology_values(key: &str) -> Option<&'static [&'static str]> {
    ATTRIBUTE_ONTOLOGY.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// R10 (port depuis parser.py, logique réelle à deux paliers) :
/// - 10 à 12 attributs : warning (k=9 dépassé, ignoré mais pas fatal)
/// - 13+ attributs : erreur (anti-bombing, attaque probable)
const ATTR_WARN_THRESHOLD: usize = 9;
const ATTR_ERROR_THRESHOLD: usize = 12;

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub code:    String,
    pub message: String,
    pub line:    usize,
}

fn attr_sigma(rel: &Relation) -> Option<f64> {
    rel.attrs.iter()
        .find(|f| f.name == "sigma" || f.name == "σ")
        .and_then(|f| f.value.parse::<f64>().ok())
}

fn attr_tau(rel: &Relation) -> Option<String> {
    rel.attrs.iter()
        .find(|f| f.name == "tau" || f.name == "τ")
        .map(|f| f.value.clone())
}

/// Prend UN SEUL slice de Relation — celles avec modality=Some(..) sont
/// traitées comme contraintes, celles avec modality=None comme relations
/// causales normales. Reflète exactement comment parser.rs les stocke déjà.
///
/// `domain` est optionnel : None = noyau seulement (comportement historique,
/// utilisé par tous les tests existants via new()). Some(d) = noyau + verbes
/// du domaine d, utilisé par with_domain() pour les payloads sectoriels.
pub struct SemanticValidator<'a> {
    all: &'a [Relation],
    domain: Option<&'a str>,
}

impl<'a> SemanticValidator<'a> {
    pub fn new(all: &'a [Relation]) -> Self {
        SemanticValidator { all, domain: None }
    }

    pub fn with_domain(all: &'a [Relation], domain: &'a str) -> Self {
        SemanticValidator { all, domain: Some(domain) }
    }

    fn constraints(&self) -> impl Iterator<Item = &Relation> {
        self.all.iter().filter(|r| r.modality.is_some())
    }

    fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.all.iter().filter(|r| r.modality.is_none())
    }

    pub fn validate(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        errors.extend(self.check_operator_whitelist());
        errors.extend(self.check_axiom_d());
        errors.extend(self.check_axiom_k_entailment());
        errors.extend(self.check_additional_diagnostics());
        errors
    }

    /// Tous les checks de `validate()` SAUF `check_operator_whitelist`
    /// (E101/W601) et `check_axiom_d` (E107) -- ceux-la sont deja branches
    /// separement sur le chemin TCP reel (respectivement
    /// `server/validator.rs::check_sdl_operator_whitelist`, en avertissement
    /// seul, et `validate_deontic_constraints`, bloquant) depuis fix19/
    /// l'audit multi-angle du 2026-09-03.
    ///
    /// `pub` depuis le 2026-09-04 (item #2 de la liste des choses a faire,
    /// trouvaille annexe en supprimant le systeme Block/AST mort): ces checks
    /// (E108/E109/E701/E703/W502/W503/R9/R10/W602/W603/W604/W605)
    /// operent tous sur `Relation` (jamais sur `Block`, contrairement a ce
    /// qui a ete retire), donc branchables sans dependre d'un parser Block
    /// qui n'a jamais existe -- mais ils etaient testes ici depuis des mois
    /// SANS JAMAIS etre appeles par le serveur reel. Brancheur:
    /// `server/validator.rs::check_extended_semantic_diagnostics`, meme
    /// politique qu'`check_operator_whitelist` (avertissement seul, jamais
    /// un rejet -- ces checks n'ont jamais ete conçus ni testes comme des
    /// motifs de rejet d'un payload en production). E703 ajoute le 2026-09-23.
    pub fn check_additional_diagnostics(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        errors.extend(self.check_temporal_contradiction());
        errors.extend(self.check_maintain_tau());
        errors.extend(self.check_amp_inh_conflict());
        errors.extend(self.check_tau_values());
        errors.extend(self.check_attribute_ontology());
        errors.extend(self.check_attribute_bombing());
        errors.extend(self.check_contradicts_symmetry());
        errors.extend(self.check_entails_transitivity());
        errors.extend(self.check_knows_calibration());
        errors.extend(self.check_doubts_calibration());
        errors.extend(self.check_temporal_pair_consistency());
        errors.extend(self.check_epistemic_antonym_consistency());
        // Ajouts 2026-10-28 (garde-fou + fermeture de dette semantique
        // ciblee, session "On fix tout") -- meme politique que tout ce qui
        // precede : avertissement/erreur selon le code, jamais branche nulle
        // part ailleurs que cette liste, donc inerte sur le vrai chemin TCP
        // tant qu'il n'est pas ici.
        errors.extend(self.check_symmetric_operator_redundancy());
        errors.extend(self.check_equals_vs_opposition());
        errors.extend(self.check_equals_transitivity());
        errors.extend(self.check_reacts_valence());
        errors.extend(self.check_involves_role());
        errors.extend(self.check_either_or_wellformed());
        errors.extend(self.check_assumes_calibration());
        errors
    }

    /// E101 — LE FIX CENTRAL. N'existe nulle part ailleurs dans le code.
    ///
    /// `pub` depuis le branchement dans le pipeline TCP reel (audit multi-angle,
    /// 2026-09-03) : server::validator::check_sdl_operator_whitelist() appelle
    /// UNIQUEMENT ce check (pas validate() en entier, qui suppose un format de
    /// bloc/valeur -- "sujet OP cible" en une seule string -- que le parser reel
    /// ne produit jamais) sur des Relation adaptees depuis un vrai CstlPayload.
    pub fn check_operator_whitelist(&self) -> Vec<SemanticError> {
        let mut errors: Vec<SemanticError> = self.all.iter()
            .filter(|r| !r.operator.is_empty())
            .filter(|r| !OFFICIAL_OPERATORS.contains(&r.operator.as_str()))
            .filter(|r| !DEPRECATED_OPERATORS.contains(&r.operator.as_str()))
            .filter(|r| !self.domain
                .map(|d| crate::domains::is_domain_operator(&r.operator, d))
                .unwrap_or(false))
            .map(|r| SemanticError {
                code: "E101".to_string(),
                message: format!(
                    "Opérateur invalide '{}' — absent des 36 opérateurs officiels (ligne {})",
                    r.operator, r.line
                ),
                line: r.line,
            })
            .collect();

        errors.extend(
            self.all.iter()
                .filter(|r| DEPRECATED_OPERATORS.contains(&r.operator.as_str()))
                .map(|r| SemanticError {
                    code: "W601".to_string(),
                    message: format!(
                        "Opérateur '{}' déprécié depuis v5.0 — migrer vers EQUALS/POSSESSES/RESEMBLES/CO_LOCATES/OPPOSES/COMPARES (ligne {})",
                        r.operator, r.line
                    ),
                    line: r.line,
                })
        );

        errors
    }

    /// E107 — Axiome D de SDL : ¬(MUST p ∧ MUST_NOT p)
    ///
    /// `pub` depuis le 2026-09-04 (meme raison que `check_operator_whitelist`
    /// ci-dessus): branchee sur le pipeline TCP reel
    /// (`server/validator.rs::validate_deontic_constraints`), qui remplace
    /// un check casse (comparaison de sous-chaines sur un seul champ
    /// `RELATION.type`, jamais capable de detecter une vraie contradiction
    /// deontique et generant meme des faux positifs sur `MUST_NOT` isole).
    pub fn check_axiom_d(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let constraints: Vec<&Relation> = self.constraints().collect();
        for c1 in &constraints {
            let Some(ref m1) = c1.modality else { continue };
            if !REQUIRED_MODALITIES.contains(&m1.as_str()) { continue }
            for c2 in &constraints {
                let Some(ref m2) = c2.modality else { continue };
                if FORBIDDEN_MODALITIES.contains(&m2.as_str())
                    && c1.subject == c2.subject
                    && c1.object == c2.object
                {
                    errors.push(SemanticError {
                        code: "E107".to_string(),
                        message: format!(
                            "SDL Axiome D violé : ({}) {} {} et ({}) {} {} coexistent (lignes {}, {})",
                            m1, c1.subject, c1.object, m2, c2.subject, c2.object, c1.line, c2.line
                        ),
                        line: c2.line,
                    });
                }
            }
        }
        errors
    }

    /// E110 — Axiome K de SDL (distributivité) sur des chaînes `ENTAILS`
    /// factuelles : O(φ→ψ) → (O(φ)→O(ψ)). `check_axiom_d` (E107) ne détecte
    /// une contradiction que quand MUST et MUST_NOT portent EXACTEMENT le
    /// même (subject, object) — un trou réel : rien ne relie aujourd'hui une
    /// obligation sur A à une interdiction sur B même quand le payload
    /// affirme lui-même, explicitement, `A ENTAILS B`.
    ///
    /// Exemple concret que E107 seul laisse passer : `(MUST) physician
    /// PRESCRIBE drug_A` + `(therapy) ENTAILS monitoring_required` +
    /// `(drug_A) ENTAILS monitoring_required` + `(MUST_NOT) physician
    /// PERFORM monitoring_required` — deux objets différents (`drug_A` vs
    /// `monitoring_required`), donc `check_axiom_d` ne voit rien, alors que
    /// le payload affirme lui-même que prescrire drug_A entraîne
    /// monitoring_required : obliger le premier tout en interdisant le
    /// second, pour le MÊME sujet, est la même contradiction que E107,
    /// simplement transportée par une chaîne ENTAILS que l'auteur a rendue
    /// explicite plutôt que par une coïncidence de (subject, object).
    ///
    /// Portée volontairement restreinte pour éviter les faux positifs :
    /// ENTAILS relie ici des OBJETS de relations à modalité (`c.object`),
    /// jamais des sujets ni des relations factuelles sans modalité; la
    /// fermeture transitive est calculée par un simple parcours en largeur
    /// sur les arêtes ENTAILS déclarées dans CE payload (pas d'historique —
    /// contrairement à `execution_lab::check_deontic_consistency_with_
    /// history`, une chaîne ENTAILS qui ne tient que sur un seul payload est
    /// déjà une auto-contradiction de l'auteur, donc bloquant comme E107,
    /// pas seulement informatif) ; et le MUST et le MUST_NOT comparés
    /// doivent partager le même `subject` — deux agents différents peuvent
    /// légitimement avoir des obligations opposées sur des propositions
    /// liées (l'un doit faire A, l'autre doit s'assurer que B n'arrive pas),
    /// ce n'est pas une contradiction d'auteur.
    pub fn check_axiom_k_entailment(&self) -> Vec<SemanticError> {
        use std::collections::{HashMap, HashSet, VecDeque};

        let mut entails_edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for r in self.relations() {
            if r.operator == "ENTAILS" {
                entails_edges.entry(r.subject.as_str()).or_default().push(r.object.as_str());
            }
        }
        if entails_edges.is_empty() {
            return Vec::new();
        }

        // Fermeture transitive par parcours en largeur depuis `start`,
        // arrêt si un cycle ENTAILS ramène sur `start` lui-même (pas notre
        // problème ici — un cycle ENTAILS pathologique n'est pas ce que ce
        // check vérifie, on l'ignore simplement pour ne pas boucler).
        let reachable = |start: &str| -> HashSet<String> {
            let mut seen: HashSet<String> = HashSet::new();
            let mut queue: VecDeque<&str> = VecDeque::new();
            queue.push_back(start);
            while let Some(node) = queue.pop_front() {
                if let Some(next) = entails_edges.get(node) {
                    for &n in next {
                        if seen.insert(n.to_string()) {
                            queue.push_back(n);
                        }
                    }
                }
            }
            seen
        };

        let mut errors = Vec::new();
        let constraints: Vec<&Relation> = self.constraints().collect();
        for c1 in &constraints {
            let Some(ref m1) = c1.modality else { continue };
            if !REQUIRED_MODALITIES.contains(&m1.as_str()) { continue }
            let entailed = reachable(c1.object.as_str());
            if entailed.is_empty() { continue }
            for c2 in &constraints {
                let Some(ref m2) = c2.modality else { continue };
                if FORBIDDEN_MODALITIES.contains(&m2.as_str())
                    && c1.subject == c2.subject
                    && c1.object != c2.object
                    && entailed.contains(c2.object.as_str())
                {
                    errors.push(SemanticError {
                        code: "E110".to_string(),
                        message: format!(
                            "SDL Axiome K violé : ({}) {} {} ENTAILS (transitivement) {}, mais ({}) {} {} l'interdit (lignes {}, {})",
                            m1, c1.subject, c1.object, c2.object, m2, c2.subject, c2.object, c1.line, c2.line
                        ),
                        line: c2.line,
                    });
                }
            }
        }
        errors
    }

    /// E108 — MUST_NOT(S,O,τ) + PERFORM(S,O,τ) au même instant
    fn check_temporal_contradiction(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for c in self.constraints() {
            let Some(ref m) = c.modality else { continue };
            if !FORBIDDEN_MODALITIES.contains(&m.as_str()) { continue }
            let c_tau = attr_tau(c);
            for r in self.relations() {
                if PERFORMED_OPERATORS.contains(&r.operator.as_str())
                    && r.subject == c.subject
                    && r.object == c.object
                    && attr_tau(r) == c_tau
                {
                    errors.push(SemanticError {
                        code: "E108".to_string(),
                        message: format!(
                            "Contradiction τ-locale : ({}) {} {} mais {} {} {} au même instant",
                            m, c.subject, c.object, r.operator, r.subject, r.object
                        ),
                        line: r.line,
                    });
                }
            }
        }
        errors
    }


    fn check_contradicts_symmetry(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut pairs: Vec<(String, String)> = Vec::new();
        for r in self.relations() {
            if r.operator != "CONTRADICTS" {
                continue;
            }
            let src = r.subject.clone();
            let tgt = r.object.clone();
            if pairs.iter().any(|(a, b)| a == &tgt && b == &src) {
                errors.push(SemanticError {
                    code: "W602".to_string(),
                    message: format!(
                        "({}) CONTRADICTS ({}) — reverse already declared; CONTRADICTS is anti-symmetric, one direction sufficient",
                        src, tgt
                    ),
                    line: r.line,
                });
            }
            pairs.push((src, tgt));
        }
        errors
    }

    /// W609 (2026-10-28) -- RESEMBLES et CO_LOCATES sont symetriques par
    /// DEFINITION du predicat lui-meme (si A ressemble a B en apparence, B
    /// ressemble a A en apparence ; si A est co-localise avec B, B est
    /// co-localise avec A) -- meme necessite logique pure que la redondance
    /// anti-symetrique deja detectee pour CONTRADICTS ci-dessus (W602), pas
    /// une regle inventee sur un usage agent mesure. Avertissement seul,
    /// jamais un rejet : declarer les deux sens n'est pas une erreur, juste
    /// redondant.
    fn check_symmetric_operator_redundancy(&self) -> Vec<SemanticError> {
        // OPPOSES ajoute 2026-10-28 : CSTL_SPEC_v5_0.md:396 le classe
        // explicitement "Symetrique" dans le MEME tableau que RESEMBLES/
        // CO_LOCATES ci-dessous -- oublie initialement ici malgre une
        // couverture deja donnee a OPPOSES par check_equals_vs_opposition
        // (E704), qui est un check DIFFERENT (conflit avec EQUALS, pas
        // redondance de sens inverse).
        const SYMMETRIC_OPERATORS: &[&str] = &["RESEMBLES", "CO_LOCATES", "OPPOSES"];
        let mut errors = Vec::new();
        for op in SYMMETRIC_OPERATORS {
            let mut pairs: Vec<(String, String)> = Vec::new();
            for r in self.relations() {
                if &r.operator != op {
                    continue;
                }
                let src = r.subject.clone();
                let tgt = r.object.clone();
                if pairs.iter().any(|(a, b)| a == &tgt && b == &src) {
                    errors.push(SemanticError {
                        code: "W609".to_string(),
                        message: format!(
                            "({}) {} ({}) — reverse already declared; {} is symmetric by definition, one direction sufficient",
                            src, op, tgt, op
                        ),
                        line: r.line,
                    });
                }
                pairs.push((src, tgt));
            }
        }
        errors
    }

    /// E704 (2026-10-28) -- EQUALS affirme une identite totale entre sujet
    /// et objet ; CONTRADICTS et OPPOSES affirment une exclusion mutuelle.
    /// Les deux a la fois sur la MEME paire est une contradiction logique
    /// pure (pas une regle de style, une impossibilite : A ne peut pas etre
    /// a la fois identique a B et en opposition avec B), independamment de
    /// tout usage agent mesure -- meme caractere que l'Axiome D deja cable.
    fn check_equals_vs_opposition(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut equals_pairs: Vec<(String, String, usize)> = Vec::new();
        let mut opposing_pairs: Vec<(String, String, &str)> = Vec::new();
        for r in self.relations() {
            if r.operator == "EQUALS" {
                equals_pairs.push((r.subject.clone(), r.object.clone(), r.line));
            } else if r.operator == "CONTRADICTS" {
                opposing_pairs.push((r.subject.clone(), r.object.clone(), "CONTRADICTS"));
            } else if r.operator == "OPPOSES" {
                opposing_pairs.push((r.subject.clone(), r.object.clone(), "OPPOSES"));
            }
        }
        for (a, b, line) in &equals_pairs {
            for (x, y, op) in &opposing_pairs {
                let same_pair = (x == a && y == b) || (x == b && y == a);
                if same_pair {
                    errors.push(SemanticError {
                        code: "E704".to_string(),
                        message: format!(
                            "({}) EQUALS ({}) but also {} declared between them — cannot be simultaneously identical and mutually exclusive",
                            a, b, op
                        ),
                        line: *line,
                    });
                }
            }
        }
        errors
    }

    /// W610 (2026-10-28) -- EQUALS est une relation d'equivalence ; la
    /// cloture transitive (A=B, B=C => A=C) devrait etre declaree
    /// explicitement pour rester verifiable sans inference cachee -- meme
    /// politique exacte que W603 (cloture ENTAILS) juste au-dessus dans ce
    /// fichier, reutilisee a l'identique pour EQUALS. Avertissement seul.
    fn check_equals_transitivity(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut equals: Vec<(String, String, usize)> = Vec::new();
        for r in self.relations() {
            if r.operator == "EQUALS" {
                equals.push((r.subject.clone(), r.object.clone(), r.line));
            }
        }
        for (a, b, line_ab) in &equals {
            for (b2, c, _) in &equals {
                if b == b2 && a != c {
                    let declared = equals.iter().any(|(x, y, _)| x == a && y == c);
                    if !declared {
                        errors.push(SemanticError {
                            code: "W610".to_string(),
                            message: format!(
                                "EQUALS closure: ({})=({})=({}) but ({})=({}) not declared (recommended)",
                                a, b, c, a, c
                            ),
                            line: *line_ab,
                        });
                    }
                }
            }
        }
        errors
    }

    /// W611 (2026-10-28) -- `REACTS` promet dans sa propre documentation
    /// d'ajout (voir AFFECTIVE_STANCE_OPERATOR plus haut) un attribut
    /// `valence=negative|positive|neutral|mixed` -- jamais verifie nulle
    /// part jusqu'ici. Ce n'est pas de la semantique inventee : c'est
    /// l'application du contrat que l'operateur s'est deja donne au moment
    /// de son ajout, meme caractere que R9 (ontologie d'attributs) pour les
    /// 8 cles generiques, juste scope a cet operateur precis.
    const REACTS_VALID_VALENCE: &'static [&'static str] = &["negative", "positive", "neutral", "mixed"];

    fn check_reacts_valence(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.all.iter() {
            if r.operator != "REACTS" {
                continue;
            }
            if let Some(f) = r.attrs.iter().find(|f| f.name == "valence") {
                if !Self::REACTS_VALID_VALENCE.contains(&f.value.as_str()) {
                    errors.push(SemanticError {
                        code: "W611".to_string(),
                        message: format!(
                            "REACTS valence='{}' hors de l'enumeration documentee {:?} (ligne {})",
                            f.value, Self::REACTS_VALID_VALENCE, f.line
                        ),
                        line: f.line,
                    });
                }
            }
        }
        errors
    }

    /// W612 (2026-10-28) -- meme logique que check_reacts_valence
    /// ci-dessus, pour `INVOLVES` et son attribut documente
    /// `role=theme|patient|duration|instrument` (voir
    /// THEMATIC_ARGUMENT_OPERATOR plus haut). `role` est optionnel (le
    /// commentaire d'ajout le dit explicitement), donc l'absence de
    /// l'attribut n'est jamais une erreur -- seulement une valeur presente
    /// mais hors de l'enumeration documentee.
    const INVOLVES_VALID_ROLE: &'static [&'static str] = &["theme", "patient", "duration", "instrument"];

    fn check_involves_role(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.all.iter() {
            if r.operator != "INVOLVES" {
                continue;
            }
            if let Some(f) = r.attrs.iter().find(|f| f.name == "role") {
                if !Self::INVOLVES_VALID_ROLE.contains(&f.value.as_str()) {
                    errors.push(SemanticError {
                        code: "W612".to_string(),
                        message: format!(
                            "INVOLVES role='{}' hors de l'enumeration documentee {:?} (ligne {})",
                            f.value, Self::INVOLVES_VALID_ROLE, f.line
                        ),
                        line: f.line,
                    });
                }
            }
        }
        errors
    }

    /// E705 (2026-10-28) -- bien-forme structurel minimal pour EITHER_OR :
    /// un sujet et un objet identiques rendent la disjonction vide de sens
    /// ("A ou A" n'est pas un choix). Pas une regle sur l'usage, une
    /// malformation evidente -- meme caractere que R10 (anti-bombing
    /// d'attributs), hygiene structurelle plutot que profondeur semantique.
    fn check_either_or_wellformed(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.relations() {
            if r.operator == "EITHER_OR" && r.subject == r.object {
                errors.push(SemanticError {
                    code: "E705".to_string(),
                    message: format!(
                        "EITHER_OR ({}, {}) — identical subject and object, degenerate disjunction (ligne {})",
                        r.subject, r.object, r.line
                    ),
                    line: r.line,
                });
            }
        }
        errors
    }

    fn check_entails_transitivity(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut entails: Vec<(String, String, usize)> = Vec::new();
        for r in self.relations() {
            if r.operator == "ENTAILS" {
                entails.push((r.subject.clone(), r.object.clone(), r.line));
            }
        }
        for (a, b, line_ab) in &entails {
            for (b2, c, _) in &entails {
                if b == b2 && a != c {
                    let declared = entails.iter().any(|(x, y, _)| x == a && y == c);
                    if !declared {
                        errors.push(SemanticError {
                            code: "W603".to_string(),
                            message: format!(
                                "ENTAILS closure: ({})->({})->({}) but ({})->({}) not declared (recommended)",
                                a, b, c, a, c
                            ),
                            line: *line_ab,
                        });
                    }
                }
            }
        }
        errors
    }

    fn check_knows_calibration(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.relations() {
            if r.operator == "KNOWS" {
                if let Some(s) = attr_sigma(r) {
                    if s < 0.8 {
                        errors.push(SemanticError {
                            code: "W604".to_string(),
                            message: format!(
                                "KNOWS with sigma={:.2} — KNOWS implies factual certainty; expected sigma >= 0.8",
                                s
                            ),
                            line: r.line,
                        });
                    }
                }
            }
        }
        errors
    }

    fn check_doubts_calibration(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.relations() {
            if r.operator == "DOUBTS" {
                if let Some(s) = attr_sigma(r) {
                    if s > 0.5 {
                        errors.push(SemanticError {
                            code: "W605".to_string(),
                            message: format!(
                                "DOUBTS with sigma={:.2} — DOUBTS implies low confidence; expected sigma <= 0.5",
                                s
                            ),
                            line: r.line,
                        });
                    }
                }
            }
        }
        errors
    }

    /// W613 (2026-10-28) -- CSTL_SPEC_v5_0.md §10.4 promet deja
    /// "ASSUMES | Hypothese de travail | Non | 0.30-0.75" dans le MEME
    /// tableau qui documente KNOWS (>=0.80, W604 ci-dessus) et DOUBTS
    /// (<=0.50, W605 juste au-dessus) -- un contrat deja pose par le spec
    /// lui-meme, jamais applique pour ASSUMES specifiquement. Meme
    /// caractere que check_reacts_valence/check_involves_role : on applique
    /// une promesse deja faite, pas une regle inventee sur un usage mesure.
    fn check_assumes_calibration(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.relations() {
            if r.operator == "ASSUMES" {
                if let Some(s) = attr_sigma(r) {
                    if !(0.30..=0.75).contains(&s) {
                        errors.push(SemanticError {
                            code: "W613".to_string(),
                            message: format!(
                                "ASSUMES with sigma={:.2} — ASSUMES implies a working hypothesis; expected sigma in [0.30, 0.75] (CSTL_SPEC_v5_0.md §10.4)",
                                s
                            ),
                            line: r.line,
                        });
                    }
                }
            }
        }
        errors
    }

    fn check_temporal_pair_consistency(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut before_pairs: Vec<(String, String, usize)> = Vec::new();
        let mut after_pairs: Vec<(String, String)> = Vec::new();
        // DURING (2026-10-28) : couvert ici plutot que dans un check
        // separe, parce que sa contradiction est avec EXACTEMENT les memes
        // paires BEFORE/AFTER deja collectees ci-dessus -- pas de la
        // speculation sur un usage agent, de la geometrie d'intervalle pure
        // (Allen interval algebra, "during" exclut structurellement
        // "before"/"after" pour la MEME paire ordonnee) : si l'intervalle de
        // A est contenu dans celui de B, A ne peut pas AUSSI etre
        // entierement avant ou apres B. Meme caractere logique que E701
        // (BEFORE vs AFTER) juste au-dessus -- necessite structurelle, pas
        // motif d'usage mesure empiriquement comme EPISTEMIC_ANTONYMS.
        let mut during_pairs: Vec<(String, String, usize)> = Vec::new();
        for r in self.relations() {
            if r.operator == "BEFORE" {
                before_pairs.push((r.subject.clone(), r.object.clone(), r.line));
            } else if r.operator == "AFTER" {
                after_pairs.push((r.subject.clone(), r.object.clone()));
            } else if r.operator == "DURING" {
                during_pairs.push((r.subject.clone(), r.object.clone(), r.line));
            }
        }
        for (a, b, line) in &before_pairs {
            if after_pairs.iter().any(|(x, y)| x == a && y == b) {
                errors.push(SemanticError {
                    code: "E701".to_string(),
                    message: format!(
                        "({}) declared both BEFORE and AFTER ({}) — temporal contradiction",
                        a, b
                    ),
                    line: *line,
                });
            }
        }
        for (a, b, line) in &during_pairs {
            if before_pairs.iter().any(|(x, y, _)| x == a && y == b) {
                errors.push(SemanticError {
                    code: "E702".to_string(),
                    message: format!(
                        "({}) declared both DURING and BEFORE ({}) — an interval contained within another cannot also entirely precede it",
                        a, b
                    ),
                    line: *line,
                });
            }
            if after_pairs.iter().any(|(x, y)| x == a && y == b) {
                errors.push(SemanticError {
                    code: "E702".to_string(),
                    message: format!(
                        "({}) declared both DURING and AFTER ({}) — an interval contained within another cannot also entirely follow it",
                        a, b
                    ),
                    line: *line,
                });
            }
        }
        errors
    }

    /// E703 (2026-09-23) — même patron que E701 (`check_temporal_pair_consistency`)
    /// mais pour les antonymes épistémiques (`EPISTEMIC_ANTONYMS`) plutôt que
    /// temporels : (sujet) BELIEVES (objet) ET (sujet) DISBELIEVES (objet)
    /// déclarés pour LA MÊME paire dans le MÊME payload est une contradiction
    /// directe — exactement le trou trouvé le 2026-09-23 (voir commentaire de
    /// `EPISTEMIC_ANTONYMS`) : avant ce check, rien ne reliait ces deux
    /// opérateurs, donc une telle contradiction passait inaperçue même quand
    /// les DEUX relations étaient présentes explicitement dans le payload.
    fn check_epistemic_antonym_consistency(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for (positive, negative) in EPISTEMIC_ANTONYMS {
            let mut positive_pairs: Vec<(String, String, usize)> = Vec::new();
            let mut negative_pairs: Vec<(String, String)> = Vec::new();
            for r in self.relations() {
                if &r.operator == positive {
                    positive_pairs.push((r.subject.clone(), r.object.clone(), r.line));
                } else if &r.operator == negative {
                    negative_pairs.push((r.subject.clone(), r.object.clone()));
                }
            }
            for (s, o, line) in &positive_pairs {
                if negative_pairs.iter().any(|(x, y)| x == s && y == o) {
                    errors.push(SemanticError {
                        code: "E703".to_string(),
                        message: format!(
                            "({}) declared both {} and {} ({}) — epistemic contradiction",
                            s, positive, negative, o
                        ),
                        line: *line,
                    });
                }
            }
        }
        errors
    }

    // `defined_entities`/`check_undefined_entity_reference` (W606) et
    // `check_coref_with` (R8) ont ete retires le 2026-09-04 (item #2 de la
    // liste des choses a faire): les deux dependaient de `self.blocks`
    // (arbre `ast::Block`), or AUCUN code de ce depot ne construit jamais de
    // `Block` hors tests -- le format reellement parse sur le fil est plat
    // (HashMap), pas un arbre de blocs. Ces deux checks etaient donc morts a
    // vie (blocks toujours vide en pratique), pas juste non branches -- voir
    // le commentaire de tete de ast.rs pour le detail complet de la
    // trouvaille.



    /// W502 — MAINTAIN avec τ=p
    fn check_maintain_tau(&self) -> Vec<SemanticError> {
        self.relations()
            .filter(|r| r.operator == "MAINTAIN")
            .filter_map(|r| {
                let tau = attr_tau(r)?;
                (tau == "p" || tau == "p_past").then(|| SemanticError {
                    code: "W502".to_string(),
                    message: format!("MAINTAIN avec τ=p sémantiquement invalide (ligne {})", r.line),
                    line: r.line,
                })
            })
            .collect()
    }

    /// E109 — AMP + INH sur même paire sujet/objet
    fn check_amp_inh_conflict(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let amps: Vec<&Relation> = self.relations().filter(|r| r.operator == "AMP").collect();
        let inhs: Vec<&Relation> = self.relations().filter(|r| r.operator == "INH").collect();
        for a in &amps {
            for i in &inhs {
                if a.subject == i.subject && a.object == i.object {
                    errors.push(SemanticError {
                        code: "E109".to_string(),
                        message: format!(
                            "Contradiction AMP/INH sur paire ({}, {}) — lignes {} et {}",
                            a.subject, a.object, a.line, i.line
                        ),
                        line: i.line,
                    });
                }
            }
        }
        errors
    }

    /// W503 — tau hors {p, n, f}
    fn check_tau_values(&self) -> Vec<SemanticError> {
        self.all.iter()
            .filter_map(|r| {
                let tau = attr_tau(r)?;
                (!VALID_TAU.contains(&tau.as_str())).then(|| SemanticError {
                    code: "W503".to_string(),
                    message: format!("Valeur τ invalide '{}' (ligne {})", tau, r.line),
                    line: r.line,
                })
            })
            .collect()
    }

    /// R9 — valeurs d'attributs hors ontologie sémantique (warning).
    fn check_attribute_ontology(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.all.iter() {
            for f in r.attrs.iter() {
                if let Some(canonical) = ontology_values(&f.name) {
                    if !canonical.contains(&f.value.as_str()) {
                        errors.push(SemanticError {
                            code: "R9".to_string(),
                            message: format!(
                                "Attribut '{}={}' hors ontologie (ligne {}). Valeurs canoniques: {:?}",
                                f.name, f.value, f.line, canonical
                            ),
                            line: f.line,
                        });
                    }
                }
            }
        }
        errors
    }

    /// R10 — anti-bombing à deux paliers : warning si 10-12 attributs,
    /// erreur fatale si 13 ou plus (comportement réel de parser.py).
    fn check_attribute_bombing(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        for r in self.all.iter() {
            let n = r.attrs.len();
            if n > ATTR_ERROR_THRESHOLD {
                errors.push(SemanticError {
                    code: "R10".to_string(),
                    message: format!(
                        "Anti-bombing : {} attributs détectés sur '{} {} {}' (ligne {}) — possible attaque, limite {}",
                        n, r.subject, r.operator, r.object, r.line, ATTR_ERROR_THRESHOLD
                    ),
                    line: r.line,
                });
            } else if n > ATTR_WARN_THRESHOLD {
                errors.push(SemanticError {
                    code: "W504".to_string(),
                    message: format!(
                        "k=9 dépassé : {} attributs sur '{} {} {}' (ligne {}) — au-delà du seuil recommandé",
                        n, r.subject, r.operator, r.object, r.line
                    ),
                    line: r.line,
                });
            }
        }
        errors
    }
}

#[cfg(test)]
mod fipa_tests {
    use super::is_fipa_performative;

    #[test]
    fn test_known_performative_recognized() {
        assert!(is_fipa_performative("PROPOSE"));
        assert!(is_fipa_performative("REQUEST"));
        assert!(is_fipa_performative("NOT_UNDERSTOOD"));
    }

    #[test]
    fn test_recognition_is_case_insensitive() {
        assert!(is_fipa_performative("propose"));
        assert!(is_fipa_performative("Refuse"));
    }

    #[test]
    fn test_existing_control_purposes_not_treated_as_fipa() {
        // agent_register/council_decision/detect_emergence restent des
        // purposes de CONTROLE de protocole, pas des performatifs FIPA --
        // ce test documente qu'ils ne se recouvrent pas par accident.
        assert!(!is_fipa_performative("agent_register"));
        assert!(!is_fipa_performative("council_decision"));
        assert!(!is_fipa_performative("detect_emergence"));
    }

    #[test]
    fn test_arbitrary_free_text_purpose_not_recognized() {
        // Purement additif: un purpose libre existant (ex. "test",
        // "communication", "smoke_test_greeting") ne doit pas etre
        // confondu avec un performatif -- comportement inchange pour eux.
        assert!(!is_fipa_performative("test"));
        assert!(!is_fipa_performative("communication"));
        assert!(!is_fipa_performative(""));
    }
}

#[cfg(test)]
mod negotiation_tests {
    use super::negotiation_status_for;

    #[test]
    fn test_refuse_and_reject_proposal_are_refused_status() {
        assert_eq!(negotiation_status_for("REFUSE"), Some("refused"));
        assert_eq!(negotiation_status_for("REJECT_PROPOSAL"), Some("refused"));
    }

    #[test]
    fn test_accept_proposal_is_accepted_status() {
        assert_eq!(negotiation_status_for("ACCEPT_PROPOSAL"), Some("accepted"));
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(negotiation_status_for("refuse"), Some("refused"));
        assert_eq!(negotiation_status_for("Accept_Proposal"), Some("accepted"));
    }

    #[test]
    fn test_propose_and_cfp_do_not_open_negotiation_status() {
        // PROPOSE/CFP sont l'OUVERTURE d'une proposition, pas sa cloture --
        // aucun statut ici, meme s'ils restent des performatifs FIPA valides
        // (is_fipa_performative reste vrai pour eux).
        assert_eq!(negotiation_status_for("PROPOSE"), None);
        assert_eq!(negotiation_status_for("CFP"), None);
    }

    #[test]
    fn test_agree_and_other_performatives_out_of_scope() {
        // AGREE porte sur une ACTION demandee (semantique FIPA distincte
        // d'ACCEPT_PROPOSAL) -- deliberement hors de ce mecanisme minimal.
        assert_eq!(negotiation_status_for("AGREE"), None);
        assert_eq!(negotiation_status_for("INFORM"), None);
        assert_eq!(negotiation_status_for("agent_register"), None);
        assert_eq!(negotiation_status_for(""), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Field;

    fn rel(subject: &str, op: &str, object: &str, sigma: f64, tau: &str, modality: Option<&str>) -> Relation {
        Relation {
            subject: subject.into(),
            operator: op.into(),
            object: object.into(),
            attrs: vec![
                Field { name: "sigma".into(), type_hint: None, value: sigma.to_string(), line: 1 },
                Field { name: "tau".into(), type_hint: None, value: tau.into(), line: 1 },
            ],
            modality: modality.map(String::from),
            line: 1,
        }
    }

    #[test]
    fn test_invalid_operator_in_relation() {
        let data = vec![rel("patient", "TRANSMIT_FAUSH", "risk", 0.85, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_valid_operator_passes() {
        let data = vec![rel("patient", "POSSESSES", "risk", 0.85, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_invalid_operator_in_constraint() {
        // Une contrainte utilise aussi `operator` pour le verbe : ex PRESCRIBE
        let data = vec![rel("physician", "PRESCRIBEZ", "drug_A", 0.92, "n", Some("MUST"))];
        let v = SemanticValidator::new(&data);
        // PRESCRIBEZ n'est même pas dans la liste officielle des 36 —
        // ceci teste que la whitelist s'applique aussi aux CONSTRAINTS
        assert!(v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_axiom_d_violation_detected() {
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("physician", "PRESCRIBE", "drug_A", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E107"),
                "Axiome D doit être détecté quand MUST et MUST_NOT partagent sujet+objet");
    }

    #[test]
    fn test_axiom_d_different_subjects_no_violation() {
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("patient", "TAKE", "drug_A", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E107"));
    }

    #[test]
    fn test_axiom_k_entailment_violation_direct() {
        // (MUST) physician PRESCRIBE drug_A + drug_A ENTAILS monitoring_required
        // + (MUST_NOT) physician PERFORM monitoring_required -- E107 seul ne
        // voit rien (objets differents: drug_A vs monitoring_required), E110 doit
        // detecter la contradiction transportee par la chaine ENTAILS explicite.
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("drug_A", "ENTAILS", "monitoring_required", 0.95, "n", None),
            rel("physician", "PERFORM", "monitoring_required", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E110"),
                "Axiome K doit etre detecte via une chaine ENTAILS directe");
    }

    #[test]
    fn test_axiom_k_entailment_violation_transitive_two_hops() {
        // therapy ENTAILS monitoring_required ENTAILS renal_monitoring --
        // la contradiction doit etre detectee meme a 2 sauts de distance.
        let data = vec![
            rel("physician", "PRESCRIBE", "therapy", 0.92, "n", Some("MUST")),
            rel("therapy", "ENTAILS", "monitoring_required", 0.92, "n", None),
            rel("monitoring_required", "ENTAILS", "renal_monitoring", 0.95, "n", None),
            rel("physician", "PERFORM", "renal_monitoring", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E110"),
                "Axiome K doit etre detecte a travers une chaine ENTAILS transitive (2 sauts)");
    }

    #[test]
    fn test_axiom_k_entailment_different_subjects_no_violation() {
        // Deux agents differents peuvent legitimement porter des obligations
        // opposees sur des propositions liees -- pas une contradiction d'auteur.
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("drug_A", "ENTAILS", "monitoring_required", 0.95, "n", None),
            rel("nurse", "PERFORM", "monitoring_required", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E110"));
    }

    #[test]
    fn test_axiom_k_entailment_no_chain_no_violation() {
        // Memes MUST/MUST_NOT sur des objets differents, mais AUCUNE relation
        // ENTAILS ne les relie -- pas de contradiction a inferer.
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("physician", "PERFORM", "monitoring_required", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E110"));
    }

    #[test]
    fn test_axiom_k_entailment_does_not_duplicate_axiom_d() {
        // MUST et MUST_NOT sur le MEME objet -- deja couvert par E107 (meme
        // objet), E110 ne doit pas re-signaler la meme paire en double (sa
        // portee explicite exclut c1.object == c2.object).
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("physician", "PRESCRIBE", "drug_A", 1.0, "n", Some("MUST_NOT")),
        ];
        let v = SemanticValidator::new(&data);
        let errors = v.validate();
        assert!(errors.iter().any(|e| e.code == "E107"));
        assert!(!errors.iter().any(|e| e.code == "E110"),
                "E110 ne doit pas dupliquer E107 sur le meme (subject, object)");
    }

    #[test]
    fn test_amp_inh_conflict() {
        let data = vec![
            rel("drug_A", "AMP", "treatment_response", 0.88, "n", None),
            rel("drug_A", "INH", "treatment_response", 0.75, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E109"));
    }

    #[test]
    fn test_epistemic_antonym_violation_detected() {
        // Trouvaille du 2026-09-23 (cstl_comprehension_test.py, item edge_001):
        // avant ce fix, BELIEVES et DISBELIEVES sur le meme (sujet, objet)
        // ne declenchaient rien -- aucun lien entre les deux operateurs.
        let data = vec![
            rel("alice", "BELIEVES", "bob_left_early", 0.80, "n", None),
            rel("alice", "DISBELIEVES", "bob_left_early", 0.80, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E703"),
                "BELIEVES et DISBELIEVES sur le meme (sujet, objet) doit etre detecte");
    }

    #[test]
    fn test_epistemic_antonym_different_subjects_no_violation() {
        let data = vec![
            rel("alice", "BELIEVES", "bob_left_early", 0.80, "n", None),
            rel("carla", "DISBELIEVES", "bob_left_early", 0.80, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E703"));
    }

    #[test]
    fn test_epistemic_antonym_different_objects_no_violation() {
        let data = vec![
            rel("alice", "BELIEVES", "bob_left_early", 0.80, "n", None),
            rel("alice", "DISBELIEVES", "carla_arrived_late", 0.80, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E703"));
    }

    #[test]
    fn test_disbelieves_passes_whitelist() {
        // DISBELIEVES est maintenant officiel (2026-09-23) -- avant ce fix,
        // il passait quand meme (E101 est un warning, jamais bloquant), mais
        // ce test verifie explicitement qu'il ne genere plus AUCUN warning.
        let data = vec![rel("alice", "DISBELIEVES", "bob_left_early", 0.80, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_either_or_passes_whitelist() {
        // EITHER_OR est officiel depuis le 2026-10-05 (trouvaille run kappa
        // v2, item complex_002) -- verifie qu'il ne genere aucun warning
        // E101, meme patron que test_disbelieves_passes_whitelist ci-dessus.
        let data = vec![rel(
            "ace_inhibitors",
            "EITHER_OR",
            "calcium_channel_blockers",
            0.80,
            "n",
            None,
        )];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_reacts_passes_whitelist() {
        // REACTS est officiel depuis le 2026-10-05 (comparaison haiku/
        // sonnet, item medium_003) -- verifie qu'il ne genere aucun warning
        // E101, meme patron que test_either_or_passes_whitelist ci-dessus.
        let data = vec![rel("employees", "REACTS", "decision", 0.80, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_involves_passes_whitelist() {
        // INVOLVES est officiel depuis le 2026-10-05 (audit des 4 items de
        // desaccord du kappa final a 3 juges -- POSSESSES surcharge par le
        // role thematique evenement/argument, ex. medium_005, medium_002,
        // edge_004, complex_001) -- meme patron que REACTS ci-dessus.
        let data = vec![rel("signing", "INVOLVES", "contract", 0.90, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_satisfies_passes_whitelist() {
        // SATISFIES est officiel depuis le 2026-10-05, meme audit --
        // POSSESSES surcharge par la satisfaction de precondition, ex.
        // complex_004 ("disclosure POSSESSES consent" -> desaccord de juge).
        let data = vec![rel("disclosure", "SATISFIES", "consent", 0.90, "n", Some("UNLESS"))];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E101"));
    }

    #[test]
    fn test_maintain_past_invalid() {
        let data = vec![rel("physician", "MAINTAIN", "audit_trace", 0.90, "p", None)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W502"));
    }

    #[test]
    fn test_clean_medical_payload_no_errors() {
        // Corrigé le 9 juillet 2026 : PRESCRIBE/TAKE n'existaient dans aucune
        // liste (ni noyau, ni domaine médical). PRESCRIBE et ADMINISTER sont
        // les vrais opérateurs du domaine médical (domains.rs). Le domaine
        // doit être précisé via with_domain() pour que ces verbes soient
        // acceptés.
        // Operateurs de domaine traduits en anglais le 2026-10-05 (etaient
        // PRESCRIRE/ADMINISTRER en francais) -- mise a jour en miroir.
        let data = vec![
            rel("physician", "PRESCRIBE", "drug_A", 0.92, "n", Some("MUST")),
            rel("patient", "ADMINISTER", "drug_A", 1.0, "n", Some("MUST_NOT")),
            rel("patient", "POSSESSES", "risk", 0.85, "n", None),
            rel("physician", "KNOWS", "diagnosis", 0.97, "n", None),
        ];
        let v = SemanticValidator::with_domain(&data, "médical");
        let hard: Vec<_> = v.validate().iter().filter(|e| e.code.starts_with('E')).cloned().collect();
        assert!(hard.is_empty(), "Payload propre : {:?}", hard);
    }

    fn rel_with_attrs(subject: &str, op: &str, object: &str, extra_attrs: Vec<(&str, &str)>) -> Relation {
        let mut attrs = vec![
            Field { name: "sigma".into(), type_hint: None, value: "0.8".into(), line: 1 },
        ];
        for (k, v) in extra_attrs {
            attrs.push(Field { name: k.into(), type_hint: None, value: v.into(), line: 1 });
        }
        Relation {
            subject: subject.into(),
            operator: op.into(),
            object: object.into(),
            attrs,
            modality: None,
            line: 1,
        }
    }

    #[test]
    fn test_r9_canonical_value_passes() {
        let data = vec![rel_with_attrs("x", "STATE", "y", vec![("polarity", "positive")])];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "R9"));
    }

    #[test]
    fn test_r9_non_canonical_value_warns() {
        let data = vec![rel_with_attrs("x", "STATE", "y", vec![("polarity", "maybe")])];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "R9"));
    }

    #[test]
    fn test_r9_unknown_key_ignored() {
        // Clé custom hors ontologie sémantique : permissif, pas de warning R9.
        let data = vec![rel_with_attrs("x", "STATE", "y", vec![("custom_key", "anything")])];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "R9"));
    }

    #[test]
    fn test_r10_under_warn_threshold_clean() {
        let extra: Vec<(&str, &str)> = (0..5).map(|_| ("modifier", "x")).collect();
        let data = vec![rel_with_attrs("x", "STATE", "y", extra)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "R10" || e.code == "W504"));
    }

    #[test]
    fn test_r10_between_9_and_12_warns_not_errors() {
        // 11 attrs custom + 1 sigma = 12 total : au-dessus de 9, au plus 12 → warning
        let extra: Vec<(&str, &str)> = (0..11).map(|_| ("modifier", "x")).collect();
        let data = vec![rel_with_attrs("x", "STATE", "y", extra)];
        let v = SemanticValidator::new(&data);
        let errs = v.validate();
        assert!(errs.iter().any(|e| e.code == "W504"), "devrait avertir (W504)");
        assert!(!errs.iter().any(|e| e.code == "R10"), "ne devrait PAS être une erreur fatale à ce niveau");
    }

    #[test]
    fn test_r10_over_12_errors() {
        let extra: Vec<(&str, &str)> = (0..15).map(|_| ("modifier", "x")).collect();
        let data = vec![rel_with_attrs("x", "STATE", "y", extra)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "R10"));
    }

    // ── 2026-10-28 : fermeture ciblee de dette semantique (session "On fix
    // tout") -- ARR.JOIN/ARR.ACCESS, DURING, EQUALS/OPPOSES, RESEMBLES/
    // CO_LOCATES, REACTS, INVOLVES, EITHER_OR. Voir le commentaire au-dessus
    // de OPERATORS_WITH_FORMAL_SEMANTICS pour le detail par operateur, et
    // celui au-dessus de GRANDFATHERED_UNDEFINED_OPERATORS pour ce qui a ete
    // delibertement ECARTE ce soir plutot que precipite.

    #[test]
    fn test_arr_join_now_covered_by_temporal_contradiction_e108() {
        // Avant le fix : ARR.JOIN n'etait pas dans PERFORMED_OPERATORS, donc
        // cette contradiction (interdit de rejoindre l'equipe au meme tau
        // que l'acte reellement performe) passait inapercue.
        let data = vec![
            rel("physician", "ARR.JOIN", "restricted_team", 0.9, "p", Some("MUST_NOT")),
            rel("physician", "ARR.JOIN", "restricted_team", 0.9, "p", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E108"));
    }

    #[test]
    fn test_arr_access_now_covered_by_temporal_contradiction_e108() {
        let data = vec![
            rel("monitor", "ARR.ACCESS", "patient_record", 0.9, "p", Some("MUST_NOT")),
            rel("monitor", "ARR.ACCESS", "patient_record", 0.9, "p", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E108"));
    }

    #[test]
    fn test_during_before_contradiction_e702() {
        let data = vec![
            rel("meeting_a", "DURING", "meeting_b", 0.9, "n", None),
            rel("meeting_a", "BEFORE", "meeting_b", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E702"));
    }

    #[test]
    fn test_during_after_contradiction_e702() {
        let data = vec![
            rel("meeting_a", "DURING", "meeting_b", 0.9, "n", None),
            rel("meeting_a", "AFTER", "meeting_b", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E702"));
    }

    #[test]
    fn test_during_alone_no_contradiction() {
        let data = vec![rel("meeting_a", "DURING", "meeting_b", 0.9, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E702"));
    }

    #[test]
    fn test_equals_contradicts_conflict_e704() {
        let data = vec![
            rel("x", "EQUALS", "y", 0.9, "n", None),
            rel("x", "CONTRADICTS", "y", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E704"));
    }

    #[test]
    fn test_equals_opposes_conflict_e704() {
        let data = vec![
            rel("x", "EQUALS", "y", 0.9, "n", None),
            rel("y", "OPPOSES", "x", 0.9, "n", None), // sens inverse, meme paire
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E704"));
    }

    #[test]
    fn test_equals_alone_no_conflict() {
        let data = vec![rel("x", "EQUALS", "y", 0.9, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E704"));
    }

    #[test]
    fn test_equals_transitivity_w610() {
        let data = vec![
            rel("a", "EQUALS", "b", 0.9, "n", None),
            rel("b", "EQUALS", "c", 0.9, "n", None),
            // (a) EQUALS (c) jamais declare -> W610
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W610"));
    }

    #[test]
    fn test_equals_transitivity_satisfied_no_warning() {
        let data = vec![
            rel("a", "EQUALS", "b", 0.9, "n", None),
            rel("b", "EQUALS", "c", 0.9, "n", None),
            rel("a", "EQUALS", "c", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W610"));
    }

    #[test]
    fn test_resembles_reverse_redundancy_w609() {
        let data = vec![
            rel("a", "RESEMBLES", "b", 0.9, "n", None),
            rel("b", "RESEMBLES", "a", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W609"));
    }

    #[test]
    fn test_co_locates_reverse_redundancy_w609() {
        let data = vec![
            rel("a", "CO_LOCATES", "b", 0.9, "n", None),
            rel("b", "CO_LOCATES", "a", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W609"));
    }

    #[test]
    fn test_resembles_one_direction_no_warning() {
        let data = vec![rel("a", "RESEMBLES", "b", 0.9, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W609"));
    }

    #[test]
    fn test_opposes_reverse_redundancy_w609() {
        // CSTL_SPEC_v5_0.md:396 classe OPPOSES "Symetrique" -- oubli
        // dans SYMMETRIC_OPERATORS corrige le meme soir.
        let data = vec![
            rel("a", "OPPOSES", "b", 0.9, "n", None),
            rel("b", "OPPOSES", "a", 0.9, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W609"));
    }

    #[test]
    fn test_reacts_valid_valence_no_warning() {
        let data = vec![rel_with_attrs("employees", "REACTS", "decision", vec![("valence", "negative")])];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W611"));
    }

    #[test]
    fn test_reacts_invalid_valence_warns_w611() {
        let data = vec![rel_with_attrs("employees", "REACTS", "decision", vec![("valence", "furious")])];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W611"));
    }

    #[test]
    fn test_reacts_no_valence_attr_no_warning() {
        // valence est documente mais pas obligatoire -- son absence n'est
        // jamais une erreur, seulement une valeur presente et hors-liste.
        let data = vec![rel("employees", "REACTS", "decision", 0.8, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W611"));
    }

    #[test]
    fn test_involves_valid_role_no_warning() {
        let data = vec![rel_with_attrs("signing", "INVOLVES", "contract", vec![("role", "theme")])];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W612"));
    }

    #[test]
    fn test_involves_invalid_role_warns_w612() {
        let data = vec![rel_with_attrs("signing", "INVOLVES", "contract", vec![("role", "bystander")])];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W612"));
    }

    #[test]
    fn test_involves_no_role_attr_no_warning() {
        let data = vec![rel("signing", "INVOLVES", "contract", 0.8, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W612"));
    }

    #[test]
    fn test_either_or_degenerate_same_subject_object_e705() {
        let data = vec![rel("alice", "EITHER_OR", "alice", 0.8, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "E705"));
    }

    #[test]
    fn test_either_or_distinct_subject_object_no_error() {
        let data = vec![rel("alice", "EITHER_OR", "bob", 0.8, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "E705"));
    }

    #[test]
    fn test_assumes_sigma_too_high_warns_w613() {
        let data = vec![rel("analyst", "ASSUMES", "market_stable", 0.90, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W613"));
    }

    #[test]
    fn test_assumes_sigma_too_low_warns_w613() {
        let data = vec![rel("analyst", "ASSUMES", "market_stable", 0.10, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W613"));
    }

    #[test]
    fn test_assumes_sigma_in_range_no_warning() {
        let data = vec![rel("analyst", "ASSUMES", "market_stable", 0.55, "n", None)];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W613"));
    }

    #[test]
    fn test_assumes_sigma_boundaries_no_warning() {
        let data = vec![
            rel("analyst", "ASSUMES", "a", 0.30, "n", None),
            rel("analyst", "ASSUMES", "b", 0.75, "n", None),
        ];
        let v = SemanticValidator::new(&data);
        assert!(!v.validate().iter().any(|e| e.code == "W613"));
    }

    // test_r8_coref_with_valid_reference_no_warning /
    // test_r8_coref_with_undefined_reference_warns ont ete retires le
    // 2026-09-04 avec check_coref_with lui-meme (voir le commentaire a sa
    // place d'origine, plus haut dans ce fichier) -- ils construisaient des
    // `ast::Block` a la main, un type que ni le tokenizer ni le parser reel
    // ne produisent jamais.

    // ── GARDE-FOU 2026-10-28 : gouvernance d'ajout d'operateur ──
    //
    // Reponse directe a la trouvaille de ce soir (REACTS/INVOLVES/SATISFIES
    // entres au catalogue sans revue independante, decides dans la meme
    // session de diagnostic qui rencontrait le trou). Voir le commentaire
    // au-dessus de `OPERATORS_WITH_FORMAL_SEMANTICS` /
    // `GRANDFATHERED_UNDEFINED_OPERATORS` pour le raisonnement complet.

    #[test]
    fn test_every_official_operator_is_classified() {
        // Tout operateur de OFFICIAL_OPERATORS doit apparaitre dans
        // EXACTEMENT une des deux listes de classification -- jamais les
        // deux (ambigu : a-t-il une semantique ou pas ?), jamais aucune
        // (c'est precisement le trou que ce garde-fou ferme : un operateur
        // ajoute a OFFICIAL_OPERATORS sans que personne ne tranche
        // explicitement son statut semantique).
        //
        // Un futur ajout a OFFICIAL_OPERATORS qui ne touche ni
        // OPERATORS_WITH_FORMAL_SEMANTICS ni GRANDFATHERED_UNDEFINED_OPERATORS
        // fait echouer CE test, donc `cargo test`, donc CI -- la seule
        // facon de faire passer un nouvel operateur est un choix EXPLICITE
        // et VISIBLE dans le diff : soit une vraie regle `check_xxx` dediee
        // (et son nom rejoint la premiere liste), soit un aveu explicite de
        // dette dans la seconde, avec justification en commentaire.
        let mut unclassified = Vec::new();
        let mut double_classified = Vec::new();
        for op in OFFICIAL_OPERATORS {
            let has_semantics = OPERATORS_WITH_FORMAL_SEMANTICS.contains(op);
            let is_grandfathered = GRANDFATHERED_UNDEFINED_OPERATORS.contains(op);
            match (has_semantics, is_grandfathered) {
                (false, false) => unclassified.push(*op),
                (true, true) => double_classified.push(*op),
                _ => {}
            }
        }
        assert!(
            unclassified.is_empty(),
            "Operateur(s) ajoute(s) a OFFICIAL_OPERATORS sans classification \
             semantique explicite -- ajoutez-les a OPERATORS_WITH_FORMAL_SEMANTICS \
             (avec un vrai check_xxx dedie) ou, a defaut, a \
             GRANDFATHERED_UNDEFINED_OPERATORS (avec justification) : {:?}",
            unclassified
        );
        assert!(
            double_classified.is_empty(),
            "Operateur(s) present(s) dans LES DEUX listes de classification \
             a la fois (ambigu — a-t-il une semantique definie ou pas ?) : {:?}",
            double_classified
        );
    }

    #[test]
    fn test_grandfathered_list_has_no_duplicates_and_no_unofficial_operators() {
        // La liste de dette ne doit reference que de VRAIS operateurs
        // officiels (sinon elle masquerait silencieusement une faute de
        // frappe plutot que de documenter une dette reelle), et chaque
        // operateur n'y apparait qu'une fois.
        let mut seen = std::collections::HashSet::new();
        for op in GRANDFATHERED_UNDEFINED_OPERATORS {
            assert!(
                OFFICIAL_OPERATORS.contains(op),
                "'{}' est dans GRANDFATHERED_UNDEFINED_OPERATORS mais absent \
                 d'OFFICIAL_OPERATORS -- faute de frappe probable",
                op
            );
            assert!(
                seen.insert(*op),
                "'{}' apparait plus d'une fois dans GRANDFATHERED_UNDEFINED_OPERATORS",
                op
            );
        }
    }

    #[test]
    fn test_formal_semantics_list_has_no_duplicates_and_no_unofficial_operators() {
        let mut seen = std::collections::HashSet::new();
        for op in OPERATORS_WITH_FORMAL_SEMANTICS {
            assert!(
                OFFICIAL_OPERATORS.contains(op),
                "'{}' est dans OPERATORS_WITH_FORMAL_SEMANTICS mais absent \
                 d'OFFICIAL_OPERATORS -- faute de frappe probable",
                op
            );
            assert!(
                seen.insert(*op),
                "'{}' apparait plus d'une fois dans OPERATORS_WITH_FORMAL_SEMANTICS",
                op
            );
        }
    }
}
