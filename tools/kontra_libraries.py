"""The library corpus: KONTRA_LIBRARIES (default /data/main/Libraries), with Kontakt/ and UVI/ under it.
Shell tools source kontra_libraries.sh, which keeps the same default."""
import os
from pathlib import Path

LIBRARIES = Path(os.environ.get("KONTRA_LIBRARIES", "/data/main/Libraries"))
KONTAKT = LIBRARIES / "Kontakt"
UVI = LIBRARIES / "UVI"
