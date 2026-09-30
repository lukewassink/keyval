Storage format:

```
[Operation - u8][key len - u64][key - utf8][val length - u64][val - utf8]
```

Operations:

- `PUT`: 0
- `DELETE`: 1

Low level api:

- `parse(&mut file) ->  StorageResponse` used by `parseOffsets` and `read`. delegates to:
  - `parsePut`
  - `parseDelete`
