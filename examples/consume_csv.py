"""Example EXTERNAL consumer; Python standard library only, not a tool dependency.

Reads exactly the CSV columns from a handoff and records what it observed.
This is a diagnostic row-count consumer, not a report that ignores failed checks.
Usage: python3 examples/consume_csv.py RUN/receipt.json NEW_OUTPUT_DIR
"""
import csv
import hashlib
import json
from pathlib import Path
import sys

manifest = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
receipt = json.loads(manifest.read_text())
actual = []
for spec in receipt['request']['inputs']:
    if spec['format'] != 'csv':
        raise ValueError('This example supports CSV only')
    path = Path(spec['path'])
    # Small demo: read bytes once, hash and parse the SAME bytes.
    content = path.read_bytes()
    digest = hashlib.sha256(content).hexdigest()
    if receipt['bindings']['input:' + spec['id']]['sha256'] != digest:
        raise ValueError('Input differs from handoff')
    import io
    rows = list(csv.DictReader(io.StringIO(content.decode('utf-8'))))
    projected = [{column: row[column] for column in spec['columns']} for row in rows]
    actual.append(dict(id=spec['id'], path=str(path), sha256=digest,
                       columns=spec['columns'], role=spec['role'], rows=len(projected)))
out.mkdir(parents=True, exist_ok=False)
(out/'native.json').write_text(json.dumps(dict(inputs=actual, consumer='diagnostic CSV row count'), indent=2)+'\n')
mappings = [dict(input=x['id'], path_pointer=f'/inputs/{n}/path',
                 sha256_pointer=f'/inputs/{n}/sha256', columns_pointer=f'/inputs/{n}/columns',
                 role_pointer=f'/inputs/{n}/role', reader_sha256_pointer=None,
                 reader_parameters_pointer=None) for n,x in enumerate(actual)]
(out/'mapping.json').write_text(json.dumps(dict(source='native.json', mappings=mappings), indent=2)+'\n')
print(json.dumps({'observed_inputs': len(actual), 'mapping': str(out/'mapping.json')}))
