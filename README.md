# TeXWord GPUI — 類 Word 即時 LaTeX 編輯器（含 MCP Server）

以 **Rust + GPUI (`gpui-component`) + `rmcp`** 打造的原生 GPU 加速所見即所得 LaTeX 書稿編輯器，針對大型多章節中文技術書稿（例如《液體火箭引擎：從原理、設計到試驗台》）設計。

## 核心功能

- **類 Word / Typora 所見即所得編輯體驗**：
  - 左側自動解析 `main.tex` 的 `\frontmatter` / `\mainmatter` / `\appendix` 章節目錄樹，點擊切換章節。
  - 中央模擬紙張畫布即時渲染：`\chapter` / `\section` 標題、段落、`\term{中}{英}` 術語膠囊、`\SI{值}{單位}`、`\ce{化學式}`、`$...$` 與 `\begin{equation}` 分數／上下標／根號／希臘字母數學式、`example` / `keypoint` / `caution` / `casestudy` / `derivation` 自訂方框。
  - 複雜巨集（如 `tikzpicture` 或未支援環境）保留為可折疊程式碼區塊，**無損寫回 `.tex`**。
  - 點擊任一區塊即可開啟內嵌多行編輯器（支援 Windows 中文 IME 輸入法），按「套用並即時渲染」立即更新畫布。
- **書稿術語與品質檢查**：
  - 自動讀取 `terms/terms.csv`，即時檢查「避免用詞（檢查用）」、`\todo{...}` 與 `\needcite`。
- **內建 MCP Server (`rmcp` 3.10)**：
  - 啟動 GUI 時同步在 `http://127.0.0.1:3847/mcp` 開啟 **Streamable HTTP MCP Server**。
  - 支援 `--mcp-stdio` 無頭模式給只支援 stdio 的客戶端呼叫。
  - 外部 AI（Antigravity、Claude、Cursor）呼叫 MCP 修改章節後，GPUI 畫面會透過內部事件通道**即時重繪同步**。

## MCP Tools 一覽

| Tool 名稱 | 用途 |
|---|---|
| `list_chapters` | 列出 `main.tex` 的所有章節與附錄 |
| `open_chapter` | 在編輯器中切換並開啟指定章節 |
| `get_active_document` | 取得目前章節的結構化區塊清單與索引 |
| `get_block` | 讀取指定區塊的 LaTeX 原始碼與 AST |
| `replace_block` | 替換指定區塊的 LaTeX 並即時同步畫面與存檔 |
| `insert_block` | 在指定索引後插入段落、標題、公式或方框 |
| `delete_block` | 刪除指定索引區塊 |
| `insert_equation` | 快速插入編號數學公式區塊 |
| `search_manuscript` | 全書所有章節關鍵字搜尋 |
| `check_terms` | 依 `terms/terms.csv` 檢查避免用詞與待補標記 |
| `compile_pdf` | 呼叫 `build.cmd -Quick` 或 `xelatex` 編譯 PDF |

## Windows 環境建置與執行

專案原始碼放在網路磁碟（例如 `z:\書\texword-gpui`），透過 `.cargo/config.toml`（不進 Git）將 `target-dir` 指向本機 `%LOCALAPPDATA%\texword-gpui\target` 以獲得最佳編譯速度。

```powershell
# 1. 安裝 Rust + MSVC C++ Build Tools，並建立本機 target-dir 與編譯
.\scripts\setup-windows.ps1

# 2. 啟動編輯器（預設嘗試開啟 z:\書\書稿，或用 --project 指定）
cargo run --release -- --project "z:\書\書稿"

# 3. 若要以純 stdio MCP Server 模式啟動：
cargo run --release -- --mcp-stdio --project "z:\書\書稿"
```

## 連接 MCP Client（Streamable HTTP）

當 `texword_app` 視窗開啟時，可將支援 Streamable HTTP 的 MCP 客戶端連到：

```json
{
  "mcpServers": {
    "texword-gpui": {
      "url": "http://127.0.0.1:3847/mcp"
    }
  }
}
```
