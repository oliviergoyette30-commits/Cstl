//! CSTL v4.9.3 — Token types and Lexer
//! Zero external dependencies. Character-by-character, no regex.

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Hashbang,    // #!CSTL v4.9.3 MODE=A
    Keyword,     // META, DISAGREEMENT_BLOCK, GAP ...
    Ident,       // bare word / value token
    LBracket,    // [
    RBracket,    // ]
    LParen,      // (
    RParen,      // )
    Equals,      // =
    Colon,       // :
    Comma,       // ,
    Newline,     // \n
    EndMarker,   // ---END---
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind:  TokenKind,
    pub value: String,
    pub line:  usize,
    pub col:   usize,
}

impl Token {
    pub fn new(kind: TokenKind, value: impl Into<String>, line: usize, col: usize) -> Self {
        Token { kind, value: value.into(), line, col }
    }
}

/// All ratified CSTL v4.9.3 keywords (Sessions #1-#4)
pub fn is_keyword(word: &str) -> bool {
    matches!(word,
        // Header
        "META" | "MODE" | "VERSION" |
        // Data blocks (Session #1 G1, Session #3 alphabetical)
        "DEFINE" | "RULE" | "RULE_TRAILER" |
        "AGREEMENT_BLOCK" | "DISAGREEMENT_BLOCK" | "DECISION" |
        "CONSTRAINT" | "UNCERTAINTY" | "DEFINE_GROUP" |
        // Dissent primitives (Session #3 0x30-0x3F)
        "AGREEMENT" | "ALTERNATIVE" | "CAUTION" | "CONCERN" |
        "DISPUTE" | "GAP" | "PARTIAL_DISPUTE" | "RECOMMEND" |
        "REJECT" | "SELF_CRITIQUE" | "STRENGTH" | "VETO" |
        "CSTLTypeError" |
        // Modalities (Session #1 G4)
        "IF" | "IFF" | "MAY" | "MUST" | "MUST_NOT" | "SHOULD" | "UNLESS" |
        "REQUIRE" | "EXPECT" |
        // META keys (Session #2 ratified, Session #4 produced_by)
        "ACTION" | "CONTINUATION_MODE" | "CONVERSATION_ID" | "DOMAIN" |
        "encoder" | "NO_PROSE" | "PARENT_HASH" | "produced_by" |
        "payload_length_bytes" | "payload_length_tokens" |
        "RESPONSE_FORMAT" | "sigma" | "TIMESTAMP" | "TURN" | "VERIFIED_BY" |
        // Type indicators (Session #2 0x50-0x57)
        "bool" | "enum" | "EXTENSION" | "float" | "hash" |
        "int" | "iso8601" | "string" |
        // Relation ops (Session #3 0x60-0x66)
        "ARR" | "EXPRESS" | "MAINTAIN" | "TRANSFORM" | "INTENT" |
        // Other
        "AS" | "@SYNC"
    )
}

// ── Lexer ─────────────────────────────────────────────────────────────────────

pub struct Lexer {
    chars: Vec<char>,
    pos:   usize,
    line:  usize,
    col:   usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos:   0,
            line:  1,
            col:   1,
        }
    }

    fn cur(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' { self.line += 1; self.col = 1; } else { self.col += 1; }
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.cur(), Some(' ') | Some('\t')) { self.advance(); }
    }

    fn read_while(&mut self, pred: impl Fn(char) -> bool) -> String {
        let mut buf = String::new();
        while let Some(ch) = self.cur() {
            if pred(ch) { buf.push(ch); self.advance(); } else { break; }
        }
        buf
    }

    fn read_to_eol(&mut self) -> String {
        let s = self.read_while(|c| c != '\n');
        s.trim().to_string()
    }

    fn rest_starts_with(&self, pat: &str) -> bool {
        let bytes: Vec<char> = pat.chars().collect();
        self.chars[self.pos..].starts_with(&bytes)
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        loop {
            let (line, col) = (self.line, self.col);

            match self.cur() {
                None => { tokens.push(Token::new(TokenKind::Eof, "", line, col)); break; }

                // Whitespace (non-newline)
                Some(' ') | Some('\t') => { self.skip_whitespace(); }

                // Newline
                Some('\n') => {
                    self.advance();
                    tokens.push(Token::new(TokenKind::Newline, "\n", line, col));
                }

                // Carriage return — skip
                Some('\r') => { self.advance(); }

                // Comment # (not hashbang)
                Some('#') if self.peek(1) != Some('!') => {
                    self.read_to_eol();
                }

                // Hashbang #!CSTL...
                Some('#') if self.peek(1) == Some('!') => {
                    let val = self.read_to_eol();
                    tokens.push(Token::new(TokenKind::Hashbang, val, line, col));
                }

                // END marker ---END---
                Some('-') if self.rest_starts_with("---END---") => {
                    for _ in 0..9 { self.advance(); }
                    tokens.push(Token::new(TokenKind::EndMarker, "---END---", line, col));
                }

                // Structural chars
                Some('[') => { self.advance(); tokens.push(Token::new(TokenKind::LBracket, "[", line, col)); }
                Some(']') => { self.advance(); tokens.push(Token::new(TokenKind::RBracket, "]", line, col)); }
                Some('(') => { self.advance(); tokens.push(Token::new(TokenKind::LParen, "(", line, col)); }
                Some(')') => { self.advance(); tokens.push(Token::new(TokenKind::RParen, ")", line, col)); }
                Some('=') => { self.advance(); tokens.push(Token::new(TokenKind::Equals, "=", line, col)); }
                Some(':') => { self.advance(); tokens.push(Token::new(TokenKind::Colon, ":", line, col)); }
                Some(',') => { self.advance(); tokens.push(Token::new(TokenKind::Comma, ",", line, col)); }

                // Word (keyword or identifier)
                Some(ch) if ch.is_alphabetic() || ch == '_' || ch == '@' => {
                    let word = self.read_while(|c| c.is_alphanumeric() || "_-./@+".contains(c));
                    let kind = if is_keyword(&word) { TokenKind::Keyword } else { TokenKind::Ident };
                    tokens.push(Token::new(kind, word, line, col));
                }

                // Number or negative
                Some(ch) if ch.is_ascii_digit() || (ch == '-' && self.peek(1).is_some_and(|c| c.is_ascii_digit())) => {
                    let word = self.read_while(|c| c.is_alphanumeric() || "._-".contains(c));
                    tokens.push(Token::new(TokenKind::Ident, word, line, col));
                }

                // Skip unknown
                _ => { self.advance(); }
            }
        }

        tokens
    }
}
