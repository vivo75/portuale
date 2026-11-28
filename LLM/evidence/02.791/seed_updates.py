"""usage: seed_updates.py ROOT UPDATES_DIR... ; records the fixture updates files as already applied in ROOT's mtimedb
(pmtest's seed_updates_record, keyed by the frozen copy's paths) so a --pretend run applies no global update to the VDB."""
import sys
sys.path.insert(0, '/home/vivo/repo/PORTUALE/pmtest/pytests-contract-suite')
import conftest
conftest.seed_updates_record(sys.argv[1], sys.argv[2:])
