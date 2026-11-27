#!/usr/bin/env python3
"""
cstl_cli.py -- la commande unique `cstl` (2026-11-27) : CSTL cle en main.

  cstl init                      config (cstl.json), cle de l'auteur, politique d'exemple
  cstl serve                     demarre le serveur avec cette config
  cstl status                    serveur joignable ? cle publique, auteur, tableau de bord
  cstl policy apply policy.cstl  publie la politique (signee, avec recu verifie)
  cstl policy show <agent>       politique effective d'un agent (cle locale requise)
  cstl agent add <nom>           cree la cle d'un agent et l'enregistre
  cstl check <agent> <action> [--amount N]   que deciderait le serveur ?
  cstl stop [all|agent:X|action:Y] [--off]    arret d'urgence (auteur)
  cstl revoke <agent>            revoque la cle d'un agent (auteur)

Les commandes existantes restent disponibles sous `cstl <nom>` : demo,
approve, gate, mcp-proxy, mcp-server, a2a-proxy, witness, deliberate.

Configuration, par ordre de priorite : options, variables d'environnement
(CSTL_OWNER, CSTL_HOST, CSTL_PORT, CSTL_DATA, CSTL_SERVER_PUBLIC_KEY), puis
`cstl.json` dans le dossier courant (ou CSTL_CONFIG), puis les valeurs par
defaut. Le serveur ecrit sa base, sa cle et son audit dans `data` (par
defaut `./cstl-data`).

Une politique est un fichier en CSTL compact (une regle par ligne, comme
`agent_support MAY refund_order <=100`), ou un fichier JSON (liste de
relations). Rien n'est devine : une ligne illisible arrete la commande.

Une cle d'agent ne se cree que par `cstl agent add` : `check` et
`policy show` refusent un agent sans cle locale, pour ne pas enregistrer
par erreur un nom qu'un vrai agent utilisera avec une autre cle.
"""
import argparse
import contextlib
import io
import json
import os
import socket
import sys
from pathlib import Path

SDK = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK))

DEFAULTS = {"owner": "owner_ops", "host": "127.0.0.1", "port": 5050, "data": "cstl-data",
            "approvers": "", "approval_actions": ""}
ENV = {"owner": "CSTL_OWNER", "host": "CSTL_HOST", "port": "CSTL_PORT", "data": "CSTL_DATA",
       "approvers": "CSTL_APPROVERS", "approval_actions": "CSTL_APPROVAL_ACTIONS"}

EXAMPLE_POLICY = """\
# Politique CSTL en compact : une regle par ligne, `#` pour un commentaire.
#   <agent> MAY|MUST|MUST_NOT <action> [<=montant | >montant] [if:fait | unless:fait] ["raison"]
# Publiez-la avec :  cstl policy apply policy.cstl
agent_support MAY refund_order <=100
agent_support MUST_NOT refund_order >100 "above 100: a human decides"
agent_support MUST_NOT wipe_db "production data"
"""

PASSTHROUGH = {
    "demo": ("cstl_demo", "main"), "approve": ("cstl_approve", "cli"), "gate": ("cstl_gate", "main"),
    "mcp-proxy": ("cstl_mcp_proxy", "cli"), "mcp-server": ("cstl_mcp_server", "cli"),
    "a2a-proxy": ("cstl_a2a", "main"), "witness": ("cstl_witness", "main"),
    "deliberate": ("cstl_deliberation", "cli"),
}


class CliError(Exception):
    pass


# ------------------------------------------------------------------ config
def config_path():
    return Path(os.environ.get("CSTL_CONFIG", "cstl.json"))


def load_config(args=None):
    cfg = dict(DEFAULTS)
    p = config_path()
    if p.is_file():
        try:
            cfg.update(json.loads(p.read_text()))
        except ValueError as e:
            raise CliError(f"{p} illisible : {e}") from None
    for k, var in ENV.items():
        if os.environ.get(var):
            cfg[k] = os.environ[var]
    for k in ("owner", "host", "port", "data"):
        v = getattr(args, k, None) if args is not None else None
        if v not in (None, ""):
            cfg[k] = v
    cfg["port"] = int(cfg["port"])
    return cfg


def server_key(cfg, explicit=None):
    """Cle publique du serveur : option, CSTL_SERVER_PUBLIC_KEY, ou le fichier
    que le serveur ecrit dans son dossier de donnees."""
    key = explicit or os.environ.get("CSTL_SERVER_PUBLIC_KEY")
    if not key:
        f = Path(cfg["data"]) / "cstl_adn.db.server_key.pub"
        if not f.is_file():
            raise CliError(f"cle publique du serveur introuvable ({f}). Le serveur a-t-il demarre "
                           "dans ce dossier ? Sinon : --server-key ou CSTL_SERVER_PUBLIC_KEY.")
        key = str(f)
    if Path(key).is_file():
        key = Path(key).read_text().strip()
    return key


def reachable(cfg):
    try:
        socket.create_connection((cfg["host"], cfg["port"]), timeout=1).close()
        return True
    except OSError:
        return False


def need_server(cfg):
    if not reachable(cfg):
        raise CliError(f"serveur CSTL injoignable sur {cfg['host']}:{cfg['port']}. Demarrez-le : cstl serve")


def key_file(name):
    return Path.home() / ".cstl" / f"agent_{name}.key"


def agent(cfg, name, create=False):
    """Un CstlAgent. Sans `create`, refuse un nom sans cle locale."""
    if not create and not key_file(name).is_file():
        raise CliError(f"pas de cle locale pour {name!r} ({key_file(name)}). Creez-la d'abord : cstl agent add {name}")
    import cstl_llm_agent as m
    with contextlib.redirect_stdout(io.StringIO()):
        return m.CstlAgent(name, "none", host=cfg["host"], port=cfg["port"])


def registered(a):
    with contextlib.redirect_stdout(io.StringIO()):
        ok = a.register()
    if not ok:
        raise CliError(f"enregistrement refuse pour {a.name!r} (cle differente deja enregistree, ou cle revoquee)")
    return a


# ------------------------------------------------------------------ politique
def read_policy(path):
    """Un fichier, ou `-` pour l'entree standard (docker compose exec -T)."""
    text = sys.stdin.read() if path == "-" else Path(path).read_text()
    if path.endswith(".json") or text.lstrip().startswith("["):
        try:
            rels = json.loads(text)
        except ValueError as e:
            raise CliError(f"{path} : JSON illisible : {e}") from None
        if not isinstance(rels, list) or not all(isinstance(r, dict) for r in rels):
            raise CliError(f"{path} : une liste de relations JSON est attendue")
        return [{k: str(v) for k, v in r.items()} for r in rels]
    import cstl_compact
    lines = [ln for ln in text.splitlines() if ln.strip() and not ln.lstrip().startswith("#")]
    try:
        return cstl_compact.decode("\n".join(lines))
    except cstl_compact.CompactError as e:
        raise CliError(f"{path} : {e}") from None


# ------------------------------------------------------------------ commandes
def cmd_init(args):
    cfg = load_config(args)
    p = config_path()
    if p.exists():
        print(f"{p} existe deja : rien n'est remplace.")
    else:
        p.write_text(json.dumps({k: cfg[k] for k in DEFAULTS}, indent=2) + "\n")
        print(f"cree {p} (auteur de la politique : {cfg['owner']}, port {cfg['port']}, donnees ./{cfg['data']})")
    pol = Path("policy.cstl")
    if pol.exists():
        print(f"{pol} existe deja : rien n'est remplace.")
    else:
        pol.write_text(EXAMPLE_POLICY)
        print(f"cree {pol} (politique d'exemple, a adapter)")
    had_key = key_file(cfg["owner"]).is_file()
    agent(cfg, cfg["owner"], create=True)
    print(f"cle de {cfg['owner']} : {key_file(cfg['owner'])}" + (" (existante)" if had_key else " (creee)"))
    print("\nEnsuite :\n  cstl serve                     # dans un autre terminal\n"
          "  cstl policy apply policy.cstl\n  cstl agent add agent_support\n"
          "  cstl check agent_support refund_order --amount 80\n  cstl status")
    return 0


def find_binary(explicit=None):
    import cstl_demo
    try:
        return cstl_demo.find_binary(explicit)
    except SystemExit as e:
        raise CliError(str(e)) from None


def cmd_serve(args):
    cfg = load_config(args)
    binary = find_binary(args.binary)
    data = Path(cfg["data"]).resolve()
    data.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env["CSTL_PORT"] = str(cfg["port"])
    env.setdefault("CSTL_POLICY_AUTHORS", cfg["owner"])
    if cfg.get("approvers"):
        env.setdefault("CSTL_APPROVERS", cfg["approvers"])
    if cfg.get("approval_actions"):
        env.setdefault("CSTL_APPROVAL_ACTIONS", cfg["approval_actions"])
    print(f"serveur CSTL : {binary}\n  donnees  {data}\n  port     {cfg['port']}\n"
          f"  auteurs  {env['CSTL_POLICY_AUTHORS']}\n  tableau  http://127.0.0.1:{env.get('CSTL_REST_PORT', '8000')}/v1/dashboard",
          flush=True)
    os.chdir(data)
    os.execve(str(binary), [str(binary)], env)


def cmd_status(args):
    cfg = load_config(args)
    up = reachable(cfg)
    print(f"serveur        {cfg['host']}:{cfg['port']} : {'joignable' if up else 'INJOIGNABLE (cstl serve)'}")
    try:
        print(f"cle publique   {server_key(cfg, args.server_key)}")
    except CliError as e:
        print(f"cle publique   inconnue : {e}")
    owner_key = key_file(cfg["owner"]).is_file()
    print(f"auteur         {cfg['owner']} : {'cle locale presente' if owner_key else 'pas de cle locale (cstl init)'}")
    if up and owner_key:
        a = registered(agent(cfg, cfg["owner"]))
        with contextlib.redirect_stdout(io.StringIO()):
            pol = a._send_signed("policy_query", "server", [], {"request_id": f"status_{os.getpid()}"})
        enforced = pol.fields.get("authors_enforced") == "true"
        print(f"auteurs        {'imposes (CSTL_POLICY_AUTHORS)' if enforced else 'NON imposes : toute regle compte'}")
    print("tableau        http://127.0.0.1:8000/v1/dashboard (sur la machine du serveur)")
    return 0 if up else 1


def cmd_policy_apply(args):
    cfg = load_config(args)
    rels = read_policy(args.file)
    if not rels:
        raise CliError(f"{args.file} : aucune regle")
    need_server(cfg)
    key = server_key(cfg, args.server_key)
    owner = registered(agent(cfg, cfg["owner"], create=True))
    import cstl_llm_agent as m
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            receipt = owner.publish_policy(rels, key)
    except m.PolicyNotConfirmed as e:
        raise CliError(f"politique non confirmee par le serveur : {e}") from None
    print(f"{len(rels)} relation(s) publiee(s) par {owner.name}, en vigueur (recu signe, policy_seq={receipt.get('policy_seq')})")
    return 0


def cmd_policy_show(args):
    cfg = load_config(args)
    need_server(cfg)
    a = registered(agent(cfg, args.agent))
    with contextlib.redirect_stdout(io.StringIO()):
        text = a.policy_text(args.format)
    print(text)
    return 0


def cmd_agent_add(args):
    cfg = load_config(args)
    need_server(cfg)
    existed = key_file(args.name).is_file()
    a = registered(agent(cfg, args.name, create=True))
    print(f"{a.name} enregistre ({'cle existante' if existed else 'nouvelle cle'} : {key_file(a.name)})")
    print(f"cle publique   {a.pub_key}")
    return 0


def cmd_check(args):
    cfg = load_config(args)
    need_server(cfg)
    a = registered(agent(cfg, args.agent))
    with contextlib.redirect_stdout(io.StringIO()):
        allowed, resp = a.request_action(args.action, args.amount)
    f = resp.fields
    print(f"{args.agent} -> {args.action}" + (f" (montant {args.amount})" if args.amount is not None else "") +
          f" : {'AUTORISE' if allowed else 'REFUSE'}")
    for k in ("reason", "rule", "approval_id", "expires_at"):
        if f.get(k):
            print(f"  {k:12} {f[k]}")
    return 0 if allowed else 2


def cmd_stop(args):
    cfg = load_config(args)
    need_server(cfg)
    owner = registered(agent(cfg, cfg["owner"]))
    import cstl_llm_agent as m
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            owner.emergency_stop(args.scope, on=not args.off, reason=args.reason)
    except m.ControlDenied as e:
        raise CliError(f"refuse par le serveur : {e}") from None
    print(f"arret d'urgence {args.scope} : {'leve' if args.off else 'ACTIF'}")
    return 0


def cmd_revoke(args):
    cfg = load_config(args)
    need_server(cfg)
    owner = registered(agent(cfg, cfg["owner"]))
    import cstl_llm_agent as m
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            owner.revoke_key(args.agent, reason=args.reason)
    except m.ControlDenied as e:
        raise CliError(f"refuse par le serveur : {e}") from None
    print(f"cle de {args.agent} revoquee. Elle ne revient qu'avec une nouvelle cle inscrite par l'auteur.")
    return 0


# ------------------------------------------------------------------ entree
def parser():
    ap = argparse.ArgumentParser(prog="cstl", description="CSTL : controle des agents IA, cle en main.",
                                 epilog="Autres commandes : " + ", ".join(PASSTHROUGH) + " (cstl <commande> --help).")
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--owner", help="auteur de la politique (defaut : cstl.json, puis owner_ops)")
    common.add_argument("--host")
    common.add_argument("--port", type=int)
    common.add_argument("--data", help="dossier des donnees du serveur (defaut ./cstl-data)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("init", parents=[common], help="config, cle de l'auteur, politique d'exemple").set_defaults(fn=cmd_init)
    p = sub.add_parser("serve", parents=[common], help="demarre le serveur")
    p.add_argument("--binary", help="chemin de cstl_parser")
    p.set_defaults(fn=cmd_serve)
    p = sub.add_parser("status", parents=[common], help="etat du serveur et de la config")
    p.add_argument("--server-key")
    p.set_defaults(fn=cmd_status)
    pol = sub.add_parser("policy", help="politique").add_subparsers(dest="pcmd", required=True)
    p = pol.add_parser("apply", parents=[common], help="publie un fichier de politique")
    p.add_argument("file", help="fichier .cstl (compact) ou .json, ou - pour l'entree standard")
    p.add_argument("--server-key")
    p.set_defaults(fn=cmd_policy_apply)
    p = pol.add_parser("show", parents=[common], help="politique effective d'un agent")
    p.add_argument("agent")
    p.add_argument("--format", default="phrases", choices=("phrases", "compact", "symboles"))
    p.set_defaults(fn=cmd_policy_show)
    ag = sub.add_parser("agent", help="agents").add_subparsers(dest="acmd", required=True)
    p = ag.add_parser("add", parents=[common], help="cree la cle d'un agent et l'enregistre")
    p.add_argument("name")
    p.set_defaults(fn=cmd_agent_add)
    p = sub.add_parser("check", parents=[common], help="demande une autorisation, comme le ferait l'agent")
    p.add_argument("agent")
    p.add_argument("action")
    p.add_argument("--amount")
    p.set_defaults(fn=cmd_check)
    p = sub.add_parser("stop", parents=[common], help="arret d'urgence (auteur)")
    p.add_argument("scope", nargs="?", default="all", help="all, agent:<nom> ou action:<motif>")
    p.add_argument("--off", action="store_true", help="lever l'arret")
    p.add_argument("--reason")
    p.set_defaults(fn=cmd_stop)
    p = sub.add_parser("revoke", parents=[common], help="revoque la cle d'un agent (auteur)")
    p.add_argument("agent")
    p.add_argument("--reason")
    p.set_defaults(fn=cmd_revoke)
    return ap


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] in PASSTHROUGH:
        mod, fn = PASSTHROUGH[argv[0]]
        sys.argv = [f"cstl-{argv[0]}", *argv[1:]]
        module = __import__(mod)
        return getattr(module, fn)() or 0
    args = parser().parse_args(argv)
    try:
        return args.fn(args) or 0
    except CliError as e:
        print(f"cstl : {e}", file=sys.stderr)
        return 1


def cli():
    sys.exit(main())


if __name__ == "__main__":
    cli()
