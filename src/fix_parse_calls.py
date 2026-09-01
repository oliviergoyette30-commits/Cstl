from pathlib import Path

TESTS_RS = Path("tests.rs")

def main():
    if not TESTS_RS.exists():
        print("❌ tests.rs introuvable")
        return

    content = TESTS_RS.read_text()
    old = "let doc = parse(&payload);"
    new = "let doc = (&payload).parse();"

    count = content.count(old)
    if count == 0:
        print("✓ Aucune occurrence — déjà corrigé")
        return

    content = content.replace(old, new)
    TESTS_RS.write_text(content)
    print(f"✓ {count} occurrences remplacées")

if __name__ == "__main__":
    main()
