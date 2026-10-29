"""Read the committed, bounded v3 audit snapshot for the public website."""
import json
from pathlib import Path

SNAPSHOT = Path(__file__).with_name('v3-facts.json')


def all_features():
    return json.loads(SNAPSHOT.read_text(encoding='utf-8'))['rows']
