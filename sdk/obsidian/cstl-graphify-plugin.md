# CSTL Graphify Obsidian Plugin

## Overview

The CSTL Graphify Obsidian Plugin enables seamless bidirectional synchronization between CSTL audit trails and Obsidian vaults. It renders graph visualizations of agent networks, deontic modalities, and audit trails directly within Obsidian.

## Features

- **Live Graph Sync**: Automatic synchronization with CSTL audit trail (configurable refresh interval)
- **Agent Network Visualization**: Interactive node-link diagram showing agent relationships
- **Deontic Modality Coloring**: Visual distinction between MUST (red), MUST_NOT (dark red), and MAY (green) commitments
- **Audit Trail Export**: Export audit entries as Obsidian markdown notes with full metadata
- **Node Filtering**: Filter graph by node type (agent, audit_entry, deontic_*)
- **Full-Text Search**: Search graph nodes and edges by content
- **Graph Traversal**: Explore connected neighborhoods up to specified depth
- **Obsidian Dataview Integration**: Generate dynamic tables from graph data

## Installation

### Option 1: Manual Installation

1. Create directory: `<vault>/.obsidian/plugins/cstl-graphify`
2. Copy plugin files to that directory
3. Enable in Settings > Community Plugins > CSTL Graphify

### Option 2: Via BRAT (Recommended)

1. Install BRAT plugin if not already installed
2. Add repository: `oliviergoyette30-commits/Cstl`
3. Navigate to Settings > BRAT > Beta Plugin List > Add Beta Plugin
4. Search "CSTL Graphify" and enable

## Configuration

Add to your `obsidian.json`:

```json
{
  "plugins": {
    "cstl-graphify": {
      "cstl_server_url": "http://127.0.0.1:5050",
      "sync_interval_seconds": 30,
      "max_audit_entries": 500,
      "max_graph_depth": 3,
      "auto_sync_enabled": true,
      "vault_path": ".obsidian/plugins/cstl-graphify/vault",
      "theme": "dark",
      "show_agent_metadata": true,
      "show_deontic_modalities": true,
      "export_format": "json"
    }
  }
}
```

## Vault Structure

After initialization, the vault creates this structure:

```
.obsidian/plugins/cstl-graphify/vault/
├── _index.md              # Graph overview + statistics
├── agents/
│   ├── alice.md           # Agent node details + relations
│   ├── bob.md
│   └── charlie.md
├── relations/
│   ├── alice_bob.md       # Relation between two agents
│   ├── bob_charlie.md
│   └── ...
├── modalities/
│   ├── deontic_1.md       # MUST commitment (red)
│   ├── deontic_2.md       # MUST_NOT restriction (dark red)
│   └── deontic_3.md       # MAY permission (green)
└── graph.json             # Raw Graphify format export
```

## Commands

### Manual Sync

- **Sync Now**: `Cmd/Ctrl + Shift + G` → Fetch latest graph from server
- **Refresh Index**: `Cmd/Ctrl + Shift + I` → Regenerate _index.md
- **Export Graph**: `Cmd/Ctrl + Shift + E` → Save graph as JSON

### Visualization

- **Open Graph View**: Ribbon icon (graph symbol) or command `Open Graph View`
- **Focus Node**: Click any node to center and expand its neighborhood
- **Filter By Type**: Right-sidebar dropdown to show/hide node types
- **Search Nodes**: Type in search box to highlight matching nodes

### Advanced

- **Traverse Graph**: Right-click node → "Explore to depth X"
- **Copy Node JSON**: Right-click node → "Copy as JSON"
- **Create Relation**: Select two agents → "Link These Agents"

## API Endpoints Used

The plugin communicates with CSTL server via REST:

```
GET /graphify/export → Full graph JSON
GET /graphify/stats → Graph statistics
GET /graphify/nodes?type=agent → Filtered nodes
GET /graphify/edges?type=sends_to → Filtered edges
GET /graphify/search?q=query → Search results
GET /graphify/traverse?start=alice&depth=2 → Traversal
POST /graphify/sync → Trigger server-side sync
```

## Deontic Modality Visualization

The plugin uses HTML5 Canvas + D3.js for graph rendering with deontic modal coloring:

```javascript
const deonticColors = {
  "MUST": "#DC143C",        // Crimson (mandatory)
  "MUST_NOT": "#8B0000",    // Dark red (forbidden)
  "MAY": "#32CD32"          // Lime green (permissible)
};

function colorNode(node) {
  if (node.metadata.deontic_type) {
    return deonticColors[node.metadata.deontic_type];
  }
  return nodeTypeColors[node.type];
}
```

## Bidirectional Sync

### Export Direction (CSTL → Obsidian)

1. Plugin queries `/graphify/export`
2. Receives JSON with nodes, edges, metadata
3. Generates markdown files in vault structure
4. Creates frontmatter with node metadata:

```yaml
---
node_id: alice
node_type: agent
created_at: 2026-09-14T10:00:00Z
deontic_must: ["consensus_protocol", "verify_signatures"]
deontic_must_not: []
deontic_may: []
relations_count: 5
---
```

### Import Direction (Obsidian → CSTL)

Manual workflow (not automatic):

1. Edit agent or relation markdown
2. Right-click → "Sync to CSTL"
3. Plugin sends updated metadata to `/graphify/update`
4. Server updates audit trail with new relation
5. Next sync pulls back annotated entry

## Example Usage

### Scenario: Query Agent Connections

```
1. Open Graph View (Cmd+Shift+G)
2. Search for "alice" in search box
3. Result: alice node highlighted + all connected edges
4. Click alice → expands neighborhood (1 hop)
5. Right-click → "Traverse to depth 2" → shows agents alice → X → Y
```

### Scenario: View Deontic Commitments

```
1. Open _index.md
2. Scroll to "MUST Commitments" section
3. See: "alice MUST consensus_protocol"
4. Click link → opens deontic_X.md node
5. View relations, status, enforcement rules
```

### Scenario: Export Subgraph

```
1. Select specific agents (Shift+Click)
2. Right-click → "Export Subgraph"
3. Choose format: JSON, Graphify, Markdown
4. Plugin generates filtered graph.json
```

## Settings UI

### Graph Rendering

- **Node Size**: Scale factor for nodes (0.5x to 2x)
- **Edge Thickness**: Width multiplier (1x to 3x)
- **Force Simulation**: Gravity (0.0 to 1.0), charge repulsion (-100 to -10)
- **Animation Speed**: Transition duration (100ms to 1000ms)

### Sync Behavior

- **Auto-Sync Interval**: Seconds between pulls (5 to 300)
- **Max Entries**: Limit audit trail size (100 to 5000)
- **Max Depth**: Default traversal depth (1 to 5)
- **Conflict Resolution**: "Server Wins" or "Manual Review"

### Display

- **Dark/Light Theme**: Automatic or manual toggle
- **Show Node Metadata**: Toggle metadata display on hover
- **Show Deontic Labels**: Toggle deontic type labels on edges
- **Compact Mode**: Reduce spacing for dense graphs

## Troubleshooting

### Plugin Not Connecting

1. Verify CSTL server running: `curl http://127.0.0.1:5050/health`
2. Check plugin log: Console tab in Developer Tools
3. Validate config in obsidian.json

### Graph Not Updating

1. Click "Sync Now" (Cmd+Shift+G)
2. Check sync_interval_seconds setting
3. Verify `/graphify/export` endpoint accessible
4. Check vault directory permissions

### Large Graph Performance

1. Reduce `max_audit_entries` (e.g., 200 instead of 500)
2. Reduce `max_graph_depth` (e.g., 2 instead of 3)
3. Disable auto-sync or increase interval
4. Use "Filter By Type" to hide audit_entry nodes

### Memory Leaks

1. Update plugin to latest version
2. Clear cache: Settings > Developer > Cache > Clear
3. Reload plugin: Cmd+Shift+P > "Reload this plugin"

## Development

### Building Plugin

```bash
npm install
npm run build
npm run release
```

### Testing

```bash
npm test
npm run test:watch
```

### Debug Mode

Set in obsidian.json:

```json
{
  "plugins": {
    "cstl-graphify": {
      "debug": true
    }
  }
}
```

Console will output sync events, API calls, and graph updates.

## Architecture

```
Obsidian Plugin
├── UI Layer (React)
│   ├── GraphView (Canvas rendering)
│   ├── NodeDetailsPanel
│   ├── SearchBox
│   └── SettingsUI
├── Sync Engine
│   ├── GraphifyAPIClient (REST calls)
│   ├── ObsidianVaultSync (file I/O)
│   └── ConflictResolver
└── Storage
    ├── Plugin state (.obsidian/plugins/cstl-graphify/state.json)
    ├── Vault files (.obsidian/plugins/cstl-graphify/vault/)
    └── Cache (IndexedDB for performance)
```

## Security Considerations

- **API Key**: If CSTL server requires auth, add to config:
  ```json
  "api_key": "sk-...",
  "api_key_header": "Authorization: Bearer"
  ```

- **Encrypted Sync**: For sensitive graphs, enable TLS:
  ```json
  "cstl_server_url": "https://cstl.example.com:5051"
  ```

- **Vault Encryption**: Obsidian's native E2E encryption protects vault files

## Limitations

- **v1.0**: No real-time push (polling only)
- **v1.0**: Import → CSTL is manual, not automatic
- **v1.0**: Graph rendering limited to 1000 nodes (performance)
- **v1.0**: No custom node shapes (circles only)

## Roadmap

- **v1.1**: Real-time WebSocket sync instead of polling
- **v1.2**: Automatic Obsidian → CSTL export without manual trigger
- **v1.3**: Custom node shapes per type
- **v1.4**: Collaborative editing (simultaneous Obsidian + CSTL edits)
- **v2.0**: Obsidian plugin API full integration

## License

MIT. Same as CSTL project.

## Support

- **Issues**: GitHub Discussions in Cstl repo
- **API Questions**: See `/server/graphify_server.rs` in main repo
- **Sync Issues**: Check `test_graphify_integration.py` for reference implementation
