#!/usr/bin/env python3
"""CI check: verify all skill files are well-formed and up to date.

Checks:
1. Each action skill has SKILL.md, EXAMPLES.md (no _shared required check here, but warns).
2. SKILL.md is under 100 lines for action skills.
3. All code fences have language identifiers (MD040).
4. No agentmemory- prefix in skill names.
5. Reference skills exist for known topics.
6. Generated files (mempalace-mcp-tools, mempalace-rest-api) are current.
"""

import os
import re
import sys
import subprocess

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
SKILLS_DIR = os.path.join(REPO_ROOT, "plugin", "skills")

# Action skills (must have EXAMPLES.md, be under 100 lines)
ACTION_SKILLS = {
    "recall", "recap", "remember", "forget", "handoff",
    "commit-context", "commit-history", "session-history",
}

# Required reference skills
REFERENCE_SKILLS = {
    "mempalace-agents", "mempalace-architecture", "mempalace-config",
    "mempalace-hooks", "mempalace-mcp-tools", "mempalace-rest-api",
    "write-mempalace-skill",
}

errors = []
warnings = []


def check_skill_dir(skill_name, path):
    skill_md = os.path.join(path, "SKILL.md")
    examples_md = os.path.join(path, "EXAMPLES.md")

    # 1. SKILL.md exists
    if not os.path.exists(skill_md):
        errors.append(f"{skill_name}: missing SKILL.md")
        return

    # 2. Frontmatter
    with open(skill_md) as f:
        content = f.read()

    if not content.startswith("---"):
        errors.append(f"{skill_name}: SKILL.md missing YAML frontmatter")

    # 3. Code fences have language identifiers (MD040)
    in_code_block = False
    for line in content.split("\n"):
        stripped = line.strip()
        if stripped.startswith("```"):
            if not in_code_block:
                # Opening fence -- must have a language identifier
                rest = stripped[3:].strip()
                if not rest:
                    errors.append(f"{skill_name}: code fence without language identifier (MD040)")
                    break
                in_code_block = True
            else:
                in_code_block = False

    # 4. No agentmemory- prefix
    if skill_name.startswith("agentmemory-"):
        errors.append(f"{skill_name}: uses agentmemory- prefix, should be mempalace-")
    if "agentmemory" in content and "agentmemory-" not in content:
        # This could be a reference, flag as warning
        warnings.append(f"{skill_name}: contains 'agentmemory' reference")

    if skill_name in ACTION_SKILLS:
        # 5. EXAMPLES.md exists for action skills
        if not os.path.exists(examples_md):
            errors.append(f"{skill_name}: missing EXAMPLES.md (required for action skills)")

        # 6. Under 100 lines
        with open(skill_md) as f:
            line_count = sum(1 for _ in f)
        if line_count > 100:
            errors.append(f"{skill_name}: SKILL.md is {line_count} lines (max 100)")

        # 7. Has quick start section
        if "## Quick start" not in content:
            warnings.append(f"{skill_name}: SKILL.md missing ## Quick start section")

        # 8. Has Why section
        if "## Why" not in content:
            warnings.append(f"{skill_name}: SKILL.md missing ## Why section")

        # 9. Has Anti-patterns section
        if "## Anti-patterns" not in content:
            warnings.append(f"{skill_name}: SKILL.md missing ## Anti-patterns section")

    if skill_name in REFERENCE_SKILLS:
        # Reference skills should not have EXAMPLES.md
        if os.path.exists(examples_md):
            warnings.append(f"{skill_name}: reference skill has EXAMPLES.md (should be SKILL.md only)")


def main():
    if not os.path.isdir(SKILLS_DIR):
        errors.append(f"Skills directory not found: {SKILLS_DIR}")
        print_results()
        sys.exit(1)

    # Check all skill directories
    checked = set()
    for entry in os.listdir(SKILLS_DIR):
        if entry.startswith("_"):
            continue
        skill_path = os.path.join(SKILLS_DIR, entry)
        if os.path.isdir(skill_path):
            check_skill_dir(entry, skill_path)
            checked.add(entry)

    # Check that all action skills exist
    for skill in ACTION_SKILLS:
        if skill not in checked:
            errors.append(f"{skill}: action skill directory missing")

    # Check that all reference skills exist
    for skill in REFERENCE_SKILLS:
        if skill not in checked:
            errors.append(f"{skill}: reference skill directory missing")

    # Check generated files
    gen_script = os.path.join(REPO_ROOT, "scripts", "skills", "generate.py")
    if os.path.exists(gen_script):
        result = subprocess.run(
            [sys.executable, gen_script, "--check"],
            capture_output=True, text=True, cwd=REPO_ROOT
        )
        if result.returncode != 0:
            errors.append(f"Generated skills out of date: {result.stderr.strip()}")

    print_results()


def print_results():
    for w in warnings:
        print(f"WARN: {w}")
    for e in errors:
        print(f"ERROR: {e}", file=sys.stderr)

    if errors:
        print(f"\n{len(errors)} error(s), {len(warnings)} warning(s)", file=sys.stderr)
        sys.exit(1)
    else:
        print(f"All checks passed ({len(warnings)} warnings)")


if __name__ == "__main__":
    main()
