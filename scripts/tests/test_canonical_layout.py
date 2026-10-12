import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_canonical_layout_script_passes() -> None:
    proc = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "check_canonical_layout.py")],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stderr or proc.stdout
