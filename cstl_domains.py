"""
CSTL v4.0 — Ontologies de domaine
18 domaines avec opérateurs fixes et types d'entités
Auteur : Olivier Goyette + Claude Sonnet 4
Date   : 27 avril 2026
"""

# Structure d'une ontologie de domaine :
# {
#   "operators": set de verbes spécifiques au domaine
#   "entity_types": set de types d'entités spécifiques
#   "description": str
# }

DOMAIN_ONTOLOGIES = {

    # ============================================================
    # DOMAINES VALIDÉS EMPIRIQUEMENT
    # ============================================================

    "diplomatique": {
        "description": "Négociations, traités, relations internationales",
        "operators": {
            "NÉGOCIER", "NEGOCIER",
            "RATIFIER",
            "SIGNER",
            "DIVULGUER", "DIVULGUE",
            "SANCTIONNER",
            "MÉDIER", "MEDIER",
            "PROTESTER",
            "RECONNAÎTRE", "RECONNAITRE",
            "OBTENIR", "OBTAIN",
            "EXPULSER",
            "RAPPELER",
        },
        "entity_types": {
            "état", "nation", "délégation", "delegation",
            "ambassadeur", "traité", "traite",
            "accord", "protocole", "sommet"
        }
    },

    "juridique": {
        "description": "Contrats, obligations légales, contentieux",
        "operators": {
            "CONTESTER",
            "RÉSILIER", "RESILIER",
            "NOTIFIER",
            "ESTER",
            "PLAIDER",
            "CONDAMNER",
            "ACQUITTER",
            "DIVULGUER", "DIVULGUE",
            "SIGNER",
            "OBTENIR", "OBTAIN",
            "MANDATER",
            "RÉCLAMER", "RECLAMER",
        },
        "entity_types": {
            "avocat", "juge", "tribunal", "contrat",
            "clause", "partie", "demandeur", "défendeur",
            "jugement", "arrêt", "loi", "règlement"
        }
    },

    "médical": {
        "description": "Soins, diagnostics, prescriptions médicales",
        "operators": {
            "PRESCRIRE",
            "DIAGNOSTIQUER",
            "CONTRA_INDIQUER",
            "ADMINISTRER",
            "OPÉRER", "OPERER",
            "SURVEILLER",
            "RÉFÉRER", "REFERER",
            "HOSPITALISER",
            "TRAITER",
            "VACCINER",
            "PRÉVENIR", "PREVENIR",
        },
        "entity_types": {
            "patient", "médecin", "medecin",
            "infirmier", "hôpital", "hopital",
            "médicament", "medicament",
            "diagnostic", "ordonnance",
            "symptôme", "symptome", "pathologie"
        }
    },

    "corporate": {
        "description": "Gouvernance d'entreprise, management, stratégie",
        "operators": {
            "APPROUVER",
            "REJETER",
            "DÉLÉGUER", "DELEGUER",
            "REPORTER",
            "BUDGÉTER", "BUDGETER",
            "AUDITER",
            "FUSIONNER",
            "ACQUÉRIR", "ACQUERIR",
            "LICENCIER",
            "RECRUTER",
            "ÉVALUER", "EVALUER",
        },
        "entity_types": {
            "CEO", "DG", "directeur", "manager",
            "conseil", "actionnaire", "filiale",
            "département", "departement", "équipe", "equipe",
            "budget", "objectif", "KPI"
        }
    },

    "archéologique": {
        "description": "Fouilles, découvertes, patrimoine archéologique",
        "operators": {
            "DÉCOUVRIR", "DECOUVRIR",
            "FOUILLER",
            "DATER",
            "CATALOGUER",
            "PRÉSERVER", "PRESERVER",
            "PUBLIER",
            "CONTESTER",
            "ATTRIBUER",
            "RESTAURER",
            "EXCAVÉR", "EXCAVER",
        },
        "entity_types": {
            "archéologue", "archeologue",
            "site", "artefact", "strate",
            "sépulture", "sepulture",
            "datation", "fouille", "musée", "musee"
        }
    },

    "astronomique": {
        "description": "Observations, découvertes et phénomènes astronomiques",
        "operators": {
            "OBSERVER",
            "DÉTECTER", "DETECTER",
            "MESURER",
            "CATALOGUER",
            "NOMMER",
            "CONFIRMER",
            "RÉFUTER", "REFUTER",
            "PUBLIER",
            "SIMULER",
        },
        "entity_types": {
            "observatoire", "télescope", "telescope",
            "étoile", "etoile", "planète", "planete",
            "galaxie", "nébuleuse", "nebuleuse",
            "astronome", "spectre", "orbite"
        }
    },

    # ============================================================
    # DOMAINES MANQUANTS — PRIORITÉ CRITIQUE
    # ============================================================

    "financier": {
        "description": "Finance, investissement, marchés, M&A",
        "operators": {
            "INVESTIR",
            "LIQUIDER",
            "AUDITER",
            "GARANTIR",
            "COUVRIR",
            "FINANCER",
            "EMPRUNTER",
            "REMBOURSER",
            "ÉVALUER", "EVALUER",
            "ACQUÉRIR", "ACQUERIR",
            "CÉDER", "CEDER",
            "CONSOLIDER",
            "PROVISIONNER",
        },
        "entity_types": {
            "investisseur", "emprunteur", "prêteur", "preteur",
            "actif", "passif", "action", "obligation",
            "fonds", "portefeuille", "risk", "rendement",
            "banque", "auditeur", "bilan", "flux"
        }
    },

    "cyber_securite": {
        "description": "Cybersécurité, menaces, défense informatique",
        "operators": {
            "BREACH",
            "PATCH",
            "MONITOR",
            "ALERT",
            "CHIFFRER",
            "DÉCHIFFRER", "DECHIFFRER",
            "AUTHENTIFIER",
            "BLOQUER",
            "DÉTECTER", "DETECTER",
            "NEUTRALISER",
            "PATCHER",
            "SURVEILLER",
            "EXFILTRER",
            "COMPROMETTRE",
        },
        "entity_types": {
            "attaquant", "défenseur", "defenseur",
            "système", "systeme", "réseau", "reseau",
            "pare-feu", "firewall", "honeypot",
            "vulnérabilité", "vulnerabilite",
            "malware", "ransomware", "CVE", "SIEM"
        }
    },

    "reglementaire": {
        "description": "Conformité réglementaire, AI Act, RGPD, normes",
        "operators": {
            "CERTIFIER",
            "SANCTIONNER",
            "NOTIFIER",
            "ABROGER",
            "HOMOLOGUER",
            "CONTRÔLER", "CONTROLER",
            "AUTORISER",
            "INTERDIRE",
            "DÉCLARER", "DECLARER",
            "AUDITER",
            "CONFORMER",
            "REPORTER",
        },
        "entity_types": {
            "régulateur", "regulateur",
            "autorité", "autorite",
            "organisme", "norme",
            "directive", "règlement", "reglement",
            "sanction", "amende", "certification",
            "DPO", "RSSI", "auditeur"
        }
    },

    # ============================================================
    # DOMAINES MANQUANTS — PRIORITÉ HAUTE
    # ============================================================

    "supply_chain": {
        "description": "Chaîne d'approvisionnement, logistique, transport",
        "operators": {
            "LIVRER",
            "ROUTER",
            "BLOQUER",
            "SOURCER",
            "TRACER",
            "STOCKER",
            "EXPÉDIER", "EXPEDIER",
            "RÉCEPTIONNER", "RECEPTIONNER",
            "RETOURNER",
            "APPROUVER",
            "COMMANDER",
        },
        "entity_types": {
            "fournisseur", "transporteur", "entrepôt", "entrepot",
            "commande", "stock", "livraison",
            "SKU", "délai", "delai", "rupture",
            "conteneur", "douane", "incoterm"
        }
    },

    "rh": {
        "description": "Ressources humaines, recrutement, évaluation",
        "operators": {
            "RECRUTER",
            "ÉVALUER", "EVALUER",
            "LICENCIER",
            "PROMOUVOIR",
            "FORMER",
            "MUTER",
            "RÉMUNÉRER", "REMUNERER",
            "SANCTIONNER",
            "INTÉGRER", "INTEGRER",
            "OFFBOARDER",
        },
        "entity_types": {
            "candidat", "employé", "employe",
            "manager", "RH", "poste",
            "contrat", "salaire", "formation",
            "évaluation", "evaluation",
            "objectif", "compétence", "competence"
        }
    },

    # ============================================================
    # DOMAINES MANQUANTS — PRIORITÉ MOYENNE
    # ============================================================

    "recherche": {
        "description": "Recherche scientifique, publication, validation",
        "operators": {
            "HYPOTHÉSER", "HYPOTHESER",
            "VALIDER",
            "RÉFUTER", "REFUTER",
            "PUBLIER",
            "CITER",
            "REPRODUIRE",
            "RÉTRACTER", "RETRACTER",
            "FINANCER",
            "COLLABORER",
            "SOUMETTRE",
        },
        "entity_types": {
            "chercheur", "laboratoire",
            "hypothèse", "hypothese",
            "expérience", "experience",
            "publication", "journal", "peer_review",
            "données", "donnees", "résultat", "resultat"
        }
    },

    "marketing": {
        "description": "Marketing, acquisition, fidélisation, campagnes",
        "operators": {
            "CIBLER",
            "SEGMENTER",
            "CONVERTIR",
            "FIDÉLISER", "FIDELISER",
            "ACTIVER",
            "DÉSACTIVER", "DESACTIVER",
            "PERSONNALISER",
            "MESURER",
            "TESTER",
            "OPTIMISER",
        },
        "entity_types": {
            "client", "prospect", "segment",
            "campagne", "canal", "message",
            "KPI", "conversion", "ROI",
            "audience", "funnel", "lead"
        }
    },

    "immobilier": {
        "description": "Transactions immobilières, location, évaluation",
        "operators": {
            "ACQUÉRIR", "ACQUERIR",
            "LOUER",
            "HYPOTHÉQUER", "HYPOTHEQUER",
            "ÉVALUER", "EVALUER",
            "VENDRE",
            "GÉRER", "GERER",
            "RÉNOVER", "RENOVER",
            "RÉSILIER", "RESILIER",
            "NOTARIER",
        },
        "entity_types": {
            "propriétaire", "proprietaire",
            "locataire", "agent", "notaire",
            "bien", "immeuble", "lot",
            "bail", "hypothèque", "hypotheque",
            "valeur", "loyer", "charges"
        }
    },

    "assurance": {
        "description": "Assurance, sinistres, souscription, indemnisation",
        "operators": {
            "SOUSCRIRE",
            "INDEMNISER",
            "RÉSILIER", "RESILIER",
            "EXPERTISER",
            "DÉCLARER", "DECLARER",
            "COUVRIR",
            "EXCLURE",
            "REMBOURSER",
            "ÉVALUER", "EVALUER",
        },
        "entity_types": {
            "assuré", "assure", "assureur",
            "police", "sinistre", "prime",
            "franchise", "indemnité", "indemnite",
            "expert", "garantie", "risque"
        }
    },

    # ============================================================
    # DOMAINES MANQUANTS — PRIORITÉ BASSE
    # ============================================================

    "education": {
        "description": "Enseignement, évaluation, certification académique",
        "operators": {
            "ENSEIGNER",
            "ÉVALUER", "EVALUER",
            "CERTIFIER",
            "ORIENTER",
            "INSCRIRE",
            "EXCLURE",
            "DÉLIBÉRER", "DELIBERER",
            "VALIDER",
            "NOTER",
        },
        "entity_types": {
            "étudiant", "etudiant",
            "enseignant", "professeur",
            "établissement", "etablissement",
            "cours", "programme", "diplôme", "diplome",
            "note", "jury", "stage"
        }
    },

    "journalisme": {
        "description": "Journalisme, fact-checking, publication",
        "operators": {
            "SOURCER",
            "VÉRIFIER", "VERIFIER",
            "PUBLIER",
            "RECTIFIER",
            "ENQUÊTER", "ENQUETER",
            "CITER",
            "RÉVÉLER", "REVELER",
            "DÉMENTIR", "DEMENTIR",
            "COMMENTER",
        },
        "entity_types": {
            "journaliste", "rédaction", "redaction",
            "source", "article", "publication",
            "audience", "rubrique", "éditeur", "editeur",
            "fact_check", "scoop", "embargo"
        }
    },

    "energie": {
        "description": "Production, distribution et stockage d'énergie",
        "operators": {
            "PRODUIRE",
            "DISTRIBUER",
            "STOCKER",
            "TARIFER",
            "CONNECTER",
            "DÉCONNECTER", "DECONNECTER",
            "RÉGULER", "REGULER",
            "OPTIMISER",
            "PRÉVOIR", "PREVOIR",
        },
        "entity_types": {
            "producteur", "distributeur",
            "consommateur", "réseau", "reseau",
            "centrale", "panneau", "éolienne", "eolienne",
            "batterie", "compteur", "tarif", "MWh"
        }
    },
}


def get_domain_operators(domain: str) -> set:
    """Retourne les opérateurs d'un domaine, ou set vide si inconnu."""
    d = DOMAIN_ONTOLOGIES.get(domain.lower(), {})
    return d.get("operators", set())


def get_domain_entity_types(domain: str) -> set:
    """Retourne les types d'entités d'un domaine."""
    d = DOMAIN_ONTOLOGIES.get(domain.lower(), {})
    return d.get("entity_types", set())


def is_valid_operator(operator: str, domain: str = None) -> bool:
    """
    Vérifie si un opérateur est valide.
    Valide si dans les 21 officiels OU dans les opérateurs du domaine.
    """
    from cstl_parser import OPERATORS_FIXED
    if operator in OPERATORS_FIXED:
        return True
    if domain:
        return operator in get_domain_operators(domain)
    return False


def list_domains() -> list:
    """Liste tous les domaines disponibles."""
    return sorted(DOMAIN_ONTOLOGIES.keys())


def domain_info(domain: str) -> dict:
    """Retourne les infos complètes d'un domaine."""
    d = DOMAIN_ONTOLOGIES.get(domain.lower())
    if not d:
        return {"error": f"Domaine inconnu: {domain}"}
    return {
        "domain": domain,
        "description": d["description"],
        "operators": sorted(d["operators"]),
        "entity_types": sorted(d["entity_types"]),
        "operator_count": len(d["operators"]),
    }


if __name__ == "__main__":
    print(f"Domaines disponibles ({len(DOMAIN_ONTOLOGIES)}):")
    for d in list_domains():
        info = domain_info(d)
        print(f"  {d:20s} — {info['operator_count']} opérateurs — {info['description']}")

    print(f"\nTest is_valid_operator:")
    print(f"  RESIST (base)         : {is_valid_operator('RESIST')}")
    print(f"  DIVULGUE (diplo)      : {is_valid_operator('DIVULGUE', 'diplomatique')}")
    print(f"  PRESCRIRE (médical)   : {is_valid_operator('PRESCRIRE', 'médical')}")
    print(f"  BREACH (cyber)        : {is_valid_operator('BREACH', 'cyber_securite')}")
    print(f"  INVENTER (inexistant) : {is_valid_operator('INVENTER', 'juridique')}")
