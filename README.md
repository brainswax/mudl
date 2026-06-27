# mudl
Yet Another MUD Language

# Project Structure
mudl/
├── README.md
├── ARCHITECTURE.md          # High-level diagrams + decisions
├── LANGUAGE.md              # Formal language spec (we'll build this together)
├── docs/                    # Generated + hand-written documentation
├── src/                     # Rust core
│   ├── core/                # Engine, object model, interpreter
│   ├── gateway/             # Auth/RBAC layer
│   ├── irc/                 # IRC bot frontend
│   ├── repl/                # Interactive prompt
│   └── loaders/             # File + GitHub loader
├── mudl/                    # DSL examples, grammar, test cases
├── agents/                  # Task definitions, agent prompts, parallel work tracking
├── .github/
│   ├── workflows/           # CI/CD
│   └── ISSUE_TEMPLATE/      # For consistent task creation
└── tests/
