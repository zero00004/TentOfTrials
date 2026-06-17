#!/usr/bin/env python3
"""Verify that every file containing 'legacy' also contains 'LEGACY' in a comment."""
import os
import sys

LEGACY_DIRS = ['.']
EXCLUDE_DIRS = {'.git', 'node_modules', '__pycache__', 'vendor', '.venv', 'target'}
EXTENSIONS = {'.py', '.rs', '.ts', '.tsx', '.js', '.sh', '.yaml', '.yml', '.md', '.toml'}

violations = []
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if d not in EXCLUDE_DIRS]
    for f in files:
        ext = os.path.splitext(f)[1]
        if ext not in EXTENSIONS:
            continue
        path = os.path.join(root, f)
        try:
            with open(path) as fh:
                content = fh.read()
            if 'legacy' in content.lower() and 'LEGACY' not in content:
                violations.append(path)
        except:
            pass

if violations:
    print(f"VIOLATIONS: {len(violations)} files missing LEGACY comment:")
    for v in violations:
        print(f"  {v}")
    sys.exit(1)
else:
    print("OK: All files with 'legacy' also contain 'LEGACY'.")
    sys.exit(0)
