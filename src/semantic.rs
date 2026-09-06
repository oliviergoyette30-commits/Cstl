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
//! d'un domaine (ex. PRESCRIRE en médical) en plus des 36 opérateurs du noyau.

use crate::ast::Relation;

/// Opérateurs dépréciés depuis v5.0 : encore reconnus (warning, pas erreur),
/// migration recommandée. Vivait avant dans validator_semantic.rs (module
/// supprimé le 2026-09-04, item #2 de la liste des choses a faire -- son
/// seul autre contenu etait un systeme de validation Block/AST jamais
/// branche sur le chemin TCP reel, voir ast.rs pour le detail) -- rapatrie
/// ici, seul consommateur reel restant.
pub const DEPRECATED_OPERATORS: &[&str] = &["MUTUAL"];

const OFFICIAL_OPERATORS: &[&str] = &[
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST", "AMP", "INH",
    "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
    "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES",
    "COMPARES", "ENTAILS", "CONTRADICTS",
    "KNOWS", "BELIEVES", "ASSUMES", "DOUBTS",
    "BEFORE", "AFTER", "DURING",
];

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
const PERFORMED_OPERATORS:  &[&str] = &["PERFORM", "ARR", "ARR.CREATE", "ARR.PRODUCE"];
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
    /// trouvaille annexe en supprimant le systeme Block/AST mort): ces 11
    /// checks (E108/E109/E701/W502/W503/R9/R10/W602/W603/W604/W605)
    /// operent tous sur `Relation` (jamais sur `Block`, contrairement a ce
    /// qui a ete retire), donc branchables sans dependre d'un parser Block
    /// qui n'a jamais existe -- mais ils etaient testes ici depuis des mois
    /// SANS JAMAIS etre appeles par le serveur reel. Brancheur:
    /// `server/validator.rs::check_extended_semantic_diagnostics`, meme
    /// politique qu'`check_operator_whitelist` (avertissement seul, jamais
    /// un rejet -- ces checks n'ont jamais ete conçus ni testes comme des
    /// motifs de rejet d'un payload en production).
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

    fn check_temporal_pair_consistency(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut before_pairs: Vec<(String, String, usize)> = Vec::new();
        let mut after_pairs: Vec<(String, String)> = Vec::new();
        for r in self.relations() {
            if r.operator == "BEFORE" {
                before_pairs.push((r.subject.clone(), r.object.clone(), r.line));
            } else if r.operator == "AFTER" {
                after_pairs.push((r.subject.clone(), r.object.clone()));
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
    fn test_maintain_past_invalid() {
        let data = vec![rel("physician", "MAINTAIN", "audit_trace", 0.90, "p", None)];
        let v = SemanticValidator::new(&data);
        assert!(v.validate().iter().any(|e| e.code == "W502"));
    }

    #[test]
    fn test_clean_medical_payload_no_errors() {
        // Corrigé le 9 juillet 2026 : PRESCRIBE/TAKE n'existaient dans aucune
        // liste (ni noyau, ni domaine médical qui utilise le français).
        // PRESCRIRE et ADMINISTRER sont les vrais opérateurs du domaine médical
        // (voir cstl_domains.py / domains.rs). Le domaine doit être précisé
        // via with_domain() pour que ces verbes soient acceptés.
        let data = vec![
            rel("physician", "PRESCRIRE", "drug_A", 0.92, "n", Some("MUST")),
            rel("patient", "ADMINISTRER", "drug_A", 1.0, "n", Some("MUST_NOT")),
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
    // test_r8_coref_with_valid_reference_no_warning /
    // test_r8_coref_with_undefined_reference_warns ont ete retires le
    // 2026-09-04 avec check_coref_with lui-meme (voir le commentaire a sa
    // place d'origine, plus haut dans ce fichier) -- ils construisaient des
    // `ast::Block` a la main, un type que ni le tokenizer ni le parser reel
    // ne produisent jamais.
}
