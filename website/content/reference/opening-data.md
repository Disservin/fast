# Opening data

Opening names and all 500 ECO codes come from Lichess and are bundled when Fast is built.

## Update from the app

Choose **Settings › General › Opening data › Update opening data**.

- Updates apply immediately and are kept for offline use across restarts.
- If an update fails, the existing data stays. The bundled openings are always the fallback.
- Games already in a database keep their stored ECO tags. New imports use the updated data.

## Update the bundled data

```sh
python3 scripts/update_openings.py
```

The script downloads and validates the data, records the upstream revision and runs the tests.
Rebuild Fast afterwards.
