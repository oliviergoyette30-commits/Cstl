//! src/server/connection_limits.rs — Protections anti-DoS sur le listener
//! TCP (2026-10-02, point 3 de l'audit multi-angle du 2 octobre).
//!
//! `handler.rs` a deja un garde-fou slowloris (`SOCKET_READ_TIMEOUT`,
//! timeout de lecture par connexion) et une limite de taille de payload
//! (`MAX_PAYLOAD_SIZE`). Ce qui manquait avant ce fichier, confirme par
//! grep sur tout `src/` : AUCUNE limite sur le nombre de connexions
//! simultanees (`listener.rs::accept_connections` fait un `tokio::spawn`
//! sans limite par acceptation), et `governor` (declare dans `Cargo.toml`
//! depuis le debut avec le commentaire "Rate limiting") n'etait importe
//! NULLE PART dans le depot -- dependance fantome. Un client pouvait donc
//! ouvrir un nombre illimite de connexions simultanees (epuisement de
//! memoire/file descriptors), ou en ouvrir beaucoup rapidement depuis une
//! seule IP sans jamais etre ralenti.
//!
//! Deux mecanismes independants, combines ici en un seul `ConnectionLimits`
//! pour que `listener.rs` n'ait qu'un objet a consulter avant de spawn:
//!
//! 1. **Plafond global de connexions simultanees** (`tokio::sync::Semaphore`)
//!    -- un `OwnedSemaphorePermit` est acquis AVANT le spawn et deplace
//!    dans la tache spawnee; il est automatiquement libere quand la
//!    connexion se termine (fin de `handle_connection`, panic inclus --
//!    `Drop` sur le permit ne depend d'aucun chemin de sortie particulier).
//! 2. **Debit par IP source** (`governor::RateLimiter` garde par `IpAddr`,
//!    backend `DashMap` -- `dashmap` est deja une dependance directe de ce
//!    depot pour `agent_discovery.rs`/registre sharde, et c'est le backend
//!    par defaut de `governor` pour un limiteur garde). Limite le NOMBRE
//!    DE NOUVELLES CONNEXIONS par minute qu'une seule IP peut ouvrir --
//!    n'affecte jamais le debit APRES la connexion (deja couvert par
//!    `SOCKET_READ_TIMEOUT`/`MAX_PAYLOAD_SIZE` dans `handler.rs`).
//!
//! Les deux seuils sont configurables par variable d'environnement, lues
//! UNE SEULE FOIS au demarrage (meme discipline que `CSTL_TLS_*`/
//! `CSTL_COLLECT_RESPONSE_CORPUS` dans `server/mod.rs::start`) -- jamais
//! relues en cours de route.

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Plafond par defaut de connexions simultanees -- tres au-dessus de toute
/// charge reelle observee cette session (meme philosophie que
/// `MAX_PAYLOAD_SIZE` dans handler.rs: une borne large qui bloque
/// l'epuisement de ressources sans gener le trafic legitime).
const DEFAULT_MAX_CONCURRENT_CONNECTIONS: usize = 2048;

/// Debit par defaut: nouvelles connexions par IP par minute. 120/min = 2/s
/// soutenu, avec un burst initial de 120 -- genereux pour une rafale
/// legitime d'agents partageant une IP (NAT/passerelle), mais borne une
/// IP qui tente d'ouvrir des milliers de connexions.
const DEFAULT_RATE_LIMIT_PER_IP_PER_MINUTE: u32 = 120;

type IpRateLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

pub struct ConnectionLimits {
    semaphore: Arc<Semaphore>,
    rate_limiter: IpRateLimiter,
}

/// Pourquoi une connexion a ete refusee AVANT meme d'atteindre
/// `handle_connection` -- distinct des rejets APRES acceptation deja geres
/// dans `handler.rs` (payload trop gros, timeout de lecture), pour que les
/// logs distinguent clairement les deux familles de protection.
pub enum RejectionReason {
    /// Plafond global de connexions simultanees atteint.
    ServerAtCapacity,
    /// Cette IP a depasse son debit de nouvelles connexions.
    RateLimited,
}

impl ConnectionLimits {
    /// Lit `CSTL_MAX_CONNECTIONS` et `CSTL_RATE_LIMIT_PER_IP_PER_MIN`
    /// (valeurs par defaut ci-dessus si absentes ou invalides -- jamais un
    /// echec de demarrage pour une variable d'environnement mal formee,
    /// meme choix que le reste de `server/mod.rs::start`).
    pub fn from_env() -> Self {
        let max_concurrent = std::env::var("CSTL_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_MAX_CONCURRENT_CONNECTIONS);

        let per_ip_per_minute = std::env::var("CSTL_RATE_LIMIT_PER_IP_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .and_then(NonZeroU32::new)
            .unwrap_or_else(|| NonZeroU32::new(DEFAULT_RATE_LIMIT_PER_IP_PER_MINUTE).expect("constante non nulle"));

        eprintln!(
            "[ConnectionLimits] max_connexions_simultanees={} debit_par_ip={}/min",
            max_concurrent, per_ip_per_minute
        );

        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            rate_limiter: RateLimiter::dashmap(Quota::per_minute(per_ip_per_minute)),
        }
    }

    /// Verifie les deux limites pour une connexion entrante et, si elle
    /// passe, retourne le permit a GARDER pour toute la duree de la
    /// connexion (le deplacer dans la tache spawnee -- il se libere tout
    /// seul au `Drop`, y compris si `handle_connection` panique).
    ///
    /// Ordre delibere: le debit par IP AVANT le semaphore -- une IP qui
    /// spam ne doit pas consommer un permit du plafond global juste pour
    /// se faire rejeter l'instant d'apres; autant la bloquer sans toucher
    /// au budget partage entre toutes les autres IP.
    pub fn try_admit(&self, ip: IpAddr) -> Result<OwnedSemaphorePermit, RejectionReason> {
        if self.rate_limiter.check_key(&ip).is_err() {
            return Err(RejectionReason::RateLimited);
        }
        self.semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| RejectionReason::ServerAtCapacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, n))
    }

    #[test]
    fn test_semaphore_cap_enforced() {
        let limits = ConnectionLimits {
            semaphore: Arc::new(Semaphore::new(2)),
            rate_limiter: RateLimiter::dashmap(Quota::per_minute(NonZeroU32::new(1000).unwrap())),
        };

        let p1 = limits.try_admit(ip(1));
        let p2 = limits.try_admit(ip(2));
        assert!(p1.is_ok() && p2.is_ok(), "les 2 premieres connexions doivent passer (plafond=2)");

        let p3 = limits.try_admit(ip(3));
        assert!(matches!(p3, Err(RejectionReason::ServerAtCapacity)), "la 3e doit etre refusee (plafond atteint)");

        drop(p1);
        let p4 = limits.try_admit(ip(4));
        assert!(p4.is_ok(), "liberer un permit doit en reouvrir un (differente IP)");
    }

    #[test]
    fn test_rate_limit_per_ip_enforced() {
        let limits = ConnectionLimits {
            semaphore: Arc::new(Semaphore::new(1000)),
            rate_limiter: RateLimiter::dashmap(Quota::per_minute(NonZeroU32::new(2).unwrap())),
        };

        let same_ip = ip(42);
        assert!(limits.try_admit(same_ip).is_ok(), "1ere connexion de cette IP doit passer");
        assert!(limits.try_admit(same_ip).is_ok(), "2e connexion (burst=2) doit passer");
        assert!(
            matches!(limits.try_admit(same_ip), Err(RejectionReason::RateLimited)),
            "3e connexion de la MEME IP dans la meme fenetre doit etre refusee"
        );
    }

    #[test]
    fn test_different_ips_have_independent_budgets() {
        let limits = ConnectionLimits {
            semaphore: Arc::new(Semaphore::new(1000)),
            rate_limiter: RateLimiter::dashmap(Quota::per_minute(NonZeroU32::new(1).unwrap())),
        };

        assert!(limits.try_admit(ip(1)).is_ok());
        assert!(
            matches!(limits.try_admit(ip(1)), Err(RejectionReason::RateLimited)),
            "IP 1 a deja epuise son budget (quota=1)"
        );
        // IP differente -- budget independant, ne doit PAS etre affectee par IP 1.
        assert!(limits.try_admit(ip(2)).is_ok(), "IP 2 doit avoir son propre budget, non partage avec IP 1");
    }
}
