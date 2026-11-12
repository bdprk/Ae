# ADR-0144: CRC-16 Integrity, ZigZag Encoding, Type Tags, and Wire Frame Headers

## Context
Under the Standard Library Blueprint (Domain E: Serialization, Encoding & Integrity), network RPCs, cached ASTs, and compact wire formats require:
1. CRC-16-IBM frame checksum generation.
2. ZigZag signed integer encoding (`zigzag_encode_i64`, `zigzag_decode_i64`) mapping negative integers to small positive integers before VarInt compression.
3. Compact binary serializer type tags (`SerializedTypeTag`, `TagNull`, `TagBool`, `TagInt`, `TagArray`, `TagMap`).
4. Wire frame headers (`WireFrameHeader`, `magic`, `payload_len`, `checksum`) for packet streaming.

## Design
- `std/core/crc16.ae`: `crc16_update_byte`.
- `std/core/varint_zigzag.ae`: `zigzag_encode_i64`, `zigzag_decode_i64`.
- `std/core/byte_tag.ae`: `SerializedTypeTag`, `type_tag_number`, `type_tag_is_container`.
- `std/core/chunk_wire.ae`: `WireFrameHeader`, `wire_frame_is_valid_magic`.
