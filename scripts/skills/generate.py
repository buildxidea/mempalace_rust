#!/usr/bin/env python3
"""Generate reference skill files (mempalace-mcp-tools, mempalace-rest-api) from Rust source.

Reads the MCP dispatch table and REST API router from mcp_server.rs / rest_api.rs
and regenerates the SKILL.md reference tables.

Usage:
    python3 scripts/skills/generate.py [--check]
"""

import argparse
import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
SKILLS_DIR = os.path.join(REPO_ROOT, "plugin", "skills")
MCP_SERVER_RS = os.path.join(REPO_ROOT, "crates", "core", "src", "mcp_server.rs")
REST_API_RS = os.path.join(REPO_ROOT, "crates", "core", "src", "rest_api.rs")

MCP_TOOLS_SKILL = os.path.join(SKILLS_DIR, "mempalace-mcp-tools", "SKILL.md")
REST_API_SKILL = os.path.join(SKILLS_DIR, "mempalace-rest-api", "SKILL.md")


def extract_mcp_tools(source_path):
    """Extract canonical mempalace_* tool names from the dispatch table."""
    if not os.path.exists(source_path):
        print(f"ERROR: {source_path} not found", file=sys.stderr)
        return []

    with open(source_path) as f:
        content = f.read()

    # Find the make_dispatch function and extract mempalace_ tool names
    # Match patterns like: "mempalace_tool_name" => tool_handler
    pattern = r'"mempalace_([a-z_]+)"\s*=>'
    matches = re.findall(pattern, content)
    return sorted(set(matches))


def extract_rest_routes(source_path):
    """Extract REST API routes from build_router."""
    if not os.path.exists(source_path):
        print(f"ERROR: {source_path} not found", file=sys.stderr)
        return []

    with open(source_path) as f:
        content = f.read()

    # Find build_router or the route definitions
    # Match patterns like: .route("/path", get(handler))
    routes = []
    pattern = r'\.route\("([^"]+)"\s*,\s*(get|post)\([^)]+\)\)'
    for m in re.finditer(pattern, content, re.DOTALL):
        routes.append((m.group(1), m.group(2).upper()))
    return routes


def generate_mcp_tools_skill(tools):
    """Generate the MCP tools reference SKILL.md content."""
    lines = []
    lines.append("---")
    lines.append("name: mempalace-mcp-tools")
    lines.append("description: Reference table of all MCP tools provided by the mempalace server")
    lines.append("---")
    lines.append("")
    lines.append(
        "The mempalace MCP server exposes %d tools. "
        "Each canonical `mempalace_*` name has a `memory_*` alias for backward compatibility." % len(tools)
    )
    lines.append("")

    # Group tools by category
    categories = {
        "Core": ["status", "list_wings", "list_rooms", "get_taxonomy", "search", "smart_search", "hybrid_search", "check_duplicate", "add_drawer", "delete_drawer", "get_aaak_spec"],
        "Knowledge graph": ["kg_query", "kg_add", "kg_invalidate", "kg_timeline", "kg_stats", "traverse", "find_tunnels", "graph_search", "graph_expand", "graph_stats"],
        "Diary": ["diary_write", "diary_read"],
        "Slots": ["slot_list", "slot_get", "slot_create", "slot_append", "slot_replace", "slot_delete"],
        "Session / commit": ["sessions", "commits", "commit_lookup"],
        "Governance": ["governance_delete", "heal", "verify"],
        "Action / frontier": ["action_create", "action_update", "frontier", "next", "lease", "routine_run", "signal_send", "signal_read"],
        "Sentinel / checkpoint": ["sentinel_create", "sentinel_trigger", "sentinel_list", "sentinel_delete", "checkpoint", "checkpoint_list", "checkpoint_resolve"],
    }

    # Remaining tools go into "Other"
    categorized = set()
    for cat_tools in categories.values():
        categorized.update("mempalace_" + t for t in cat_tools)
    other_tools = sorted(set("mempalace_" + t for t in tools) - categorized)

    for cat_name, cat_tools in categories.items():
        present = [t for t in cat_tools if "mempalace_" + t in set("mempalace_" + t for t in tools)]
        if not present:
            continue
        lines.append("## " + cat_name)
        lines.append("")
        for t in present:
            lines.append("- `mempalace_%s`" % t)
        lines.append("")

    if other_tools:
        lines.append("## Other")
        lines.append("")
        lines.append(",".join("`%s`" % t for t in other_tools))
        lines.append("")
    lines.append("")
    return "\n".join(lines)


def generate_rest_api_skill(routes):
    """Generate the REST API reference SKILL.md content."""
    lines = []
    lines.append("---")
    lines.append("name: mempalace-rest-api")
    lines.append("description: REST API endpoint reference for the mempalace HTTP server")
    lines.append("---")
    lines.append("")
    lines.append("The REST API is served on port 6969 by default (`MEMPALACE_HTTP_PORT`). Start with `mpr serve --http`.")
    lines.append("")
    lines.append("## Endpoints")
    lines.append("")
    lines.append("| Method | Path |")
    lines.append("|--------|------|")

    # Group and sort by path
    route_map = {}
    for path, method in routes:
        if path not in route_map:
            route_map[path] = []
        route_map[path].append(method)

    for path in sorted(route_map.keys()):
        methods = "/".join(sorted(route_map[path]))
        lines.append("| %s | `%s` |" % (methods, path))

    lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Generate reference skill files from Rust source")
    parser.add_argument("--check", action="store_true", help="Check if files are up to date without writing")
    args = parser.parse_args()

    tools = extract_mcp_tools(MCP_SERVER_RS)
    routes = extract_rest_routes(REST_API_RS)

    if not tools:
        print("ERROR: No MCP tools extracted -- source parsing may have failed", file=sys.stderr)
        sys.exit(1)

    mcp_content = generate_mcp_tools_skill(tools)
    rest_content = generate_rest_api_skill(routes)

    if args.check:
        # Read existing files and compare
        for path, generated in [(MCP_TOOLS_SKILL, mcp_content), (REST_API_SKILL, rest_content)]:
            if not os.path.exists(path):
                print(f"MISSING: {path} does not exist", file=sys.stderr)
                sys.exit(1)
            with open(path) as f:
                existing = f.read()
            if existing.strip() != generated.strip():
                print(f"STALE: {path} is out of date -- regenerate with generate.py", file=sys.stderr)
                sys.exit(1)
        print("All reference skill files are up to date.")
    else:
        os.makedirs(os.path.dirname(MCP_TOOLS_SKILL), exist_ok=True)
        os.makedirs(os.path.dirname(REST_API_SKILL), exist_ok=True)
        with open(MCP_TOOLS_SKILL, "w") as f:
            f.write(mcp_content)
        with open(REST_API_SKILL, "w") as f:
            f.write(rest_content)
        print(f"Wrote {MCP_TOOLS_SKILL} (%d tools)" % len(tools))
        print(f"Wrote {REST_API_SKILL} (%d routes)" % len(routes))


if __name__ == "__main__":
    main()
