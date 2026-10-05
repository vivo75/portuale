#!/usr/bin/env python3
"""
VDB corpus analyzer for #305 (feat#157).

Reads one or more VDB roots and reports:
- Entry count, file count, total size per VDB
- List of distinct file names with counts (top 40 for host)
- Unusual cases: invalid UTF-8, empty files, odd modes, stale metadata,
  -MERGING- dirs, hidden files in category dirs, non-directories at category level

Usage: python3 305-s0-corpus.py <vdb-root> [<vdb-root2> ...]
"""

import sys
import os
import subprocess
from pathlib import Path
from collections import defaultdict
import struct

def check_utf8(path):
    """Return True if the path is valid UTF-8."""
    try:
        path.encode('utf-8').decode('utf-8')
        return True
    except (UnicodeEncodeError, UnicodeDecodeError):
        return False

def read_metadata_mtime(metadata_path):
    """Extract #dir_mtime= value from metadata file, or None if not found."""
    try:
        with open(metadata_path, 'r') as f:
            for line in f:
                line = line.strip()
                if line.startswith('#dir_mtime='):
                    try:
                        return int(line.split('=', 1)[1])
                    except (ValueError, IndexError):
                        return None
    except Exception:
        pass
    return None

def get_entry_mtime_ns(entry_path):
    """Get st_mtime_ns of an entry directory."""
    try:
        stat = os.stat(entry_path)
        return stat.st_mtime_ns
    except Exception:
        return None

def analyze_vdb(vdb_root):
    """Analyze one VDB root and return stats and unusual cases."""
    vdb_path = Path(vdb_root)

    stats = {
        'path': vdb_root,
        'categories': 0,
        'entries': 0,
        'files': 0,
        'total_bytes': 0,
        'file_names': defaultdict(int),
    }

    unusual = {
        'invalid_utf8_paths': [],
        'empty_files': [],
        'odd_modes': [],
        'symlinks': [],
        'missing_metadata': [],
        'stale_metadata_mtime': [],
        'merging_dirs': [],
        'hidden_files_in_cats': [],
        'non_dirs_at_cat_level': [],
    }

    if not vdb_path.is_dir():
        print(f"Warning: {vdb_root} is not a directory", file=sys.stderr)
        return stats, unusual

    try:
        categories = sorted([d for d in os.listdir(vdb_path)
                           if (vdb_path / d).is_dir()])
    except Exception as e:
        print(f"Error reading {vdb_root}: {e}", file=sys.stderr)
        return stats, unusual

    stats['categories'] = len(categories)

    for cat in categories:
        cat_path = vdb_path / cat

        # Check for hidden files in category dir
        try:
            cat_contents = os.listdir(cat_path)
        except Exception:
            cat_contents = []

        for item in cat_contents:
            if item.startswith('.'):
                unusual['hidden_files_in_cats'].append(str(cat_path / item))
            elif not (cat_path / item).is_dir():
                unusual['non_dirs_at_cat_level'].append(str(cat_path / item))

        # Check for -MERGING- directories
        for entry_name in cat_contents:
            if entry_name.startswith('-MERGING-'):
                unusual['merging_dirs'].append(str(cat_path / entry_name))

        # Process entries
        for entry_name in cat_contents:
            entry_path = cat_path / entry_name

            # Skip non-directories
            if not entry_path.is_dir():
                continue

            stats['entries'] += 1

            # Check for invalid UTF-8 in entry name
            if not check_utf8(entry_name):
                unusual['invalid_utf8_paths'].append(str(entry_path))

            # Check for metadata file
            metadata_path = entry_path / 'metadata'
            has_metadata = metadata_path.exists()

            if not has_metadata:
                unusual['missing_metadata'].append(str(entry_path))
            else:
                # Check if metadata mtime matches directory mtime
                meta_mtime = read_metadata_mtime(metadata_path)
                dir_mtime_ns = get_entry_mtime_ns(entry_path)
                if meta_mtime is not None and dir_mtime_ns is not None:
                    if meta_mtime != dir_mtime_ns:
                        unusual['stale_metadata_mtime'].append({
                            'entry': str(entry_path),
                            'metadata_mtime': meta_mtime,
                            'dir_mtime_ns': dir_mtime_ns,
                        })

            # Process files in entry
            try:
                for fname in os.listdir(entry_path):
                    fpath = entry_path / fname

                    # Check for symlinks
                    if os.path.islink(fpath):
                        unusual['symlinks'].append(str(fpath))

                    # Check file properties
                    try:
                        st = os.stat(fpath, follow_symlinks=True)
                    except Exception:
                        continue

                    stats['files'] += 1
                    stats['total_bytes'] += st.st_size
                    stats['file_names'][fname] += 1

                    # Check for invalid UTF-8 in file name
                    if not check_utf8(fname):
                        unusual['invalid_utf8_paths'].append(str(fpath))

                    # Check for empty files
                    if st.st_size == 0:
                        unusual['empty_files'].append(str(fpath))

                    # Check file mode
                    mode = st.st_mode
                    if os.path.isfile(fpath):
                        # Regular files should be 0644
                        if (mode & 0o777) != 0o644:
                            unusual['odd_modes'].append({
                                'path': str(fpath),
                                'mode': oct(mode & 0o777),
                            })
                    elif os.path.isdir(fpath):
                        # Directories should be 0755
                        if (mode & 0o777) != 0o755:
                            unusual['odd_modes'].append({
                                'path': str(fpath),
                                'mode': oct(mode & 0o777),
                            })
            except Exception as e:
                print(f"Error processing {entry_path}: {e}", file=sys.stderr)

    return stats, unusual

    try:
        categories = sorted([d for d in os.listdir(vdb_path)
                           if (vdb_path / d).is_dir()])
    except Exception as e:
        print(f"Error reading {vdb_root}: {e}", file=sys.stderr)
        return stats, unusual

    stats['categories'] = len(categories)

    for cat in categories:
        cat_path = vdb_path / cat

        # Check for hidden files in category dir
        try:
            cat_contents = os.listdir(cat_path)
        except Exception:
            cat_contents = []

        for item in cat_contents:
            if item.startswith('.'):
                unusual['hidden_files_in_cats'].append(str(cat_path / item))
            elif not (cat_path / item).is_dir():
                unusual['non_dirs_at_cat_level'].append(str(cat_path / item))

        # Check for -MERGING- directories
        for entry_name in cat_contents:
            if entry_name.startswith('-MERGING-'):
                unusual['merging_dirs'].append(str(cat_path / entry_name))

        # Process entries
        for entry_name in cat_contents:
            entry_path = cat_path / entry_name

            # Skip non-directories
            if not entry_path.is_dir():
                continue

            stats['entries'] += 1

            # Check for invalid UTF-8 in entry name
            if not check_utf8(entry_name):
                unusual['invalid_utf8_paths'].append(str(entry_path))

            # Check for metadata file
            metadata_path = entry_path / 'metadata'
            has_metadata = metadata_path.exists()

            if not has_metadata:
                unusual['missing_metadata'].append(str(entry_path))
            else:
                # Check if metadata mtime matches directory mtime
                meta_mtime = read_metadata_mtime(metadata_path)
                dir_mtime_ns = get_entry_mtime_ns(entry_path)
                if meta_mtime is not None and dir_mtime_ns is not None:
                    if meta_mtime != dir_mtime_ns:
                        unusual['stale_metadata_mtime'].append({
                            'entry': str(entry_path),
                            'metadata_mtime': meta_mtime,
                            'dir_mtime_ns': dir_mtime_ns,
                        })

            # Process files in entry
            try:
                for fname in os.listdir(entry_path):
                    fpath = entry_path / fname

                    # Check for symlinks
                    if os.path.islink(fpath):
                        unusual['symlinks'].append(str(fpath))

                    # Check file properties
                    try:
                        st = os.stat(fpath, follow_symlinks=True)
                    except Exception:
                        continue

                    stats['files'] += 1
                    stats['total_bytes'] += st.st_size
                    stats['file_names'][fname] += 1

                    # Check for invalid UTF-8 in file name
                    if not check_utf8(fname):
                        unusual['invalid_utf8_paths'].append(str(fpath))

                    # Check for empty files
                    if st.st_size == 0:
                        unusual['empty_files'].append(str(fpath))

                    # Check file mode
                    mode = st.st_mode
                    if os.path.isfile(fpath):
                        # Regular files should be 0644
                        if (mode & 0o777) != 0o644:
                            unusual['odd_modes'].append({
                                'path': str(fpath),
                                'mode': oct(mode & 0o777),
                            })
                    elif os.path.isdir(fpath):
                        # Directories should be 0755
                        if (mode & 0o777) != 0o755:
                            unusual['odd_modes'].append({
                                'path': str(fpath),
                                'mode': oct(mode & 0o777),
                            })
            except Exception as e:
                print(f"Error processing {entry_path}: {e}", file=sys.stderr)

    # Get total size via du if we couldn't sum individual files
    # (as a sanity check)
    try:
        result = subprocess.run(
            ['du', '--apparent-size', '-s', vdb_root],
            capture_output=True, text=True, timeout=30
        )
        if result.returncode == 0:
            du_bytes = int(result.stdout.split()[0]) * 512  # du output is in 512-byte blocks
            # Use du value if it differs significantly (accounts for sparse files, etc.)
            if du_bytes != stats['total_bytes']:
                stats['total_bytes'] = du_bytes
    except Exception:
        pass

    return stats, unusual

def format_stats(stats_list):
    """Format statistics for output."""
    output = []

    for stats, unusual in stats_list:
        output.append(f"\nVDB: {stats['path']}")
        output.append(f"  Categories: {stats['categories']}")
        output.append(f"  Entries: {stats['entries']}")
        output.append(f"  Files: {stats['files']}")
        output.append(f"  Total bytes (apparent size): {stats['total_bytes']}")

        # Top file names
        if stats['file_names']:
            sorted_names = sorted(
                stats['file_names'].items(),
                key=lambda x: x[1],
                reverse=True
            )
            # Limit to 40 for output
            output.append(f"  Top file names (limit 40):")
            for fname, count in sorted_names[:40]:
                output.append(f"    {fname}: {count}")

        # Unusual cases
        if any(unusual.values()):
            output.append(f"\n  Unusual cases for {stats['path']}:")

            if unusual['invalid_utf8_paths']:
                output.append(f"    Invalid UTF-8 paths ({len(unusual['invalid_utf8_paths'])}):")
                for path in unusual['invalid_utf8_paths'][:10]:
                    output.append(f"      {path}")
                if len(unusual['invalid_utf8_paths']) > 10:
                    output.append(f"      ... and {len(unusual['invalid_utf8_paths']) - 10} more")

            if unusual['empty_files']:
                output.append(f"    Empty files ({len(unusual['empty_files'])}):")
                for path in unusual['empty_files'][:10]:
                    output.append(f"      {path}")
                if len(unusual['empty_files']) > 10:
                    output.append(f"      ... and {len(unusual['empty_files']) - 10} more")

            if unusual['odd_modes']:
                output.append(f"    Odd file modes ({len(unusual['odd_modes'])}):")
                for item in unusual['odd_modes'][:10]:
                    output.append(f"      {item['path']}: {item['mode']}")
                if len(unusual['odd_modes']) > 10:
                    output.append(f"      ... and {len(unusual['odd_modes']) - 10} more")

            if unusual['symlinks']:
                output.append(f"    Symlinks ({len(unusual['symlinks'])}):")
                for path in unusual['symlinks'][:10]:
                    output.append(f"      {path}")
                if len(unusual['symlinks']) > 10:
                    output.append(f"      ... and {len(unusual['symlinks']) - 10} more")

            if unusual['missing_metadata']:
                output.append(f"    Missing metadata files ({len(unusual['missing_metadata'])}):")
                for path in unusual['missing_metadata'][:10]:
                    output.append(f"      {path}")
                if len(unusual['missing_metadata']) > 10:
                    output.append(f"      ... and {len(unusual['missing_metadata']) - 10} more")

            if unusual['stale_metadata_mtime']:
                output.append(f"    Stale metadata mtime stamps ({len(unusual['stale_metadata_mtime'])}):")
                for item in unusual['stale_metadata_mtime'][:10]:
                    output.append(f"      {item['entry']}")
                    output.append(f"        metadata: {item['metadata_mtime']}, dir: {item['dir_mtime_ns']}")
                if len(unusual['stale_metadata_mtime']) > 10:
                    output.append(f"      ... and {len(unusual['stale_metadata_mtime']) - 10} more")

            if unusual['merging_dirs']:
                output.append(f"    -MERGING- directories ({len(unusual['merging_dirs'])}):")
                for path in unusual['merging_dirs'][:10]:
                    output.append(f"      {path}")
                if len(unusual['merging_dirs']) > 10:
                    output.append(f"      ... and {len(unusual['merging_dirs']) - 10} more")

            if unusual['hidden_files_in_cats']:
                output.append(f"    Hidden files in category dirs ({len(unusual['hidden_files_in_cats'])}):")
                for path in unusual['hidden_files_in_cats'][:10]:
                    output.append(f"      {path}")
                if len(unusual['hidden_files_in_cats']) > 10:
                    output.append(f"      ... and {len(unusual['hidden_files_in_cats']) - 10} more")

            if unusual['non_dirs_at_cat_level']:
                output.append(f"    Non-directories at category level ({len(unusual['non_dirs_at_cat_level'])}):")
                for path in unusual['non_dirs_at_cat_level'][:10]:
                    output.append(f"      {path}")
                if len(unusual['non_dirs_at_cat_level']) > 10:
                    output.append(f"      ... and {len(unusual['non_dirs_at_cat_level']) - 10} more")

    return '\n'.join(output)

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 305-s0-corpus.py <vdb-root> [<vdb-root2> ...]")
        print("Analyzes VDB structure and reports unusual cases.")
        sys.exit(1)

    vdb_roots = sys.argv[1:]
    results = []

    for root in vdb_roots:
        stats, unusual = analyze_vdb(root)
        results.append((stats, unusual))

    print(format_stats(results))

if __name__ == '__main__':
    main()
