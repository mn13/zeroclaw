# TOOLS.md

## Built-in Tools
- **shell** — Execute terminal commands (subject to security policy)
- **file_read** — Read file contents
- **file_write** — Write/edit files
- **memory_store** — Save durable context to long-term memory
- **memory_recall** — Search long-term memory
- **memory_forget** — Remove a memory entry

## Python 3 Runtime
Python 3.11 is pre-installed with a virtual environment at `/opt/venv`.
Call via the shell tool: `python3 script.py` or inline `python3 -c "..."`.

### Pre-installed libraries
| Package | Use for |
|---------|---------|
| `python-pptx` | Create/edit PowerPoint presentations |
| `python-docx` | Create/edit Word documents |
| `openpyxl` | Create/edit Excel spreadsheets |
| `pypdf` | Read/merge/split PDF files |
| `requests` | HTTP requests to APIs and web services |
| `pandas` | Tabular data manipulation and analysis |
| `beautifulsoup4` | Parse and scrape HTML/XML (`from bs4 import BeautifulSoup`) |
| `pyyaml` | Read/write YAML files (`import yaml`) |
| `Pillow` | Image creation, conversion, and processing (`from PIL import Image`) |

### Rules
- Always use `python3`, not `python`.
- Libraries are ready to import — no `pip install` needed.
- For scripts longer than a few lines, write to a `.py` file first, then execute.

## Node.js Runtime
Node 18 and npm are pre-installed. Call via the shell tool.

### Rules
- Use `node script.js` for execution, `npm install <pkg>` for additional packages.
- Prefer Python for data/document tasks; use Node for JS-specific needs.

## CLI Utilities
| Command | Use for |
|---------|---------|
| `rg` (ripgrep) | Fast recursive text search across files |
| `jq` | Parse, filter, and transform JSON data |

---
*Add whatever helps you do your job. This is your cheat sheet.*
