# RunWatchStreamV2 Contract

## Request

```json
{
  "run_id": "run-123",
  "after_durable_sequence": 42,
  "after_live_sequence": 120
}
```

## ACK

第一帧必须是普通 RPC Response：

```json
{
  "success": true,
  "data": {
    "stream": "run.watch",
    "streamVersion": 2
  }
}
```

## Event Frame

```json
{
  "frame_type": "event",
  "lane": "durable",
  "run_id": "run-123",
  "durable_sequence": 43,
  "live_sequence": null,
  "event_type": "message_completed",
  "payload": {},
  "timestamp": "..."
}
```

Live：

```json
{
  "frame_type": "event",
  "lane": "live",
  "run_id": "run-123",
  "durable_sequence": null,
  "live_sequence": 121,
  "event_type": "text_delta",
  "payload": {"text": "..."},
  "timestamp": "..."
}
```

## Heartbeat

```json
{
  "frame_type": "heartbeat",
  "run_id": "run-123",
  "durable_sequence": 43,
  "live_sequence": 130,
  "timestamp": "..."
}
```

## ResyncRequired

```json
{
  "frame_type": "resync_required",
  "run_id": "run-123",
  "lane": "live",
  "reason": "live_buffer_gap"
}
```

## Ordering

- Durable lane 单调递增且可 replay。
- Live lane 单调递增但只保证当前 Daemon 生命周期内的 bounded replay。
- 不要求 live/durable 共用同一 sequence。
- Renderer 不得用 live cursor 推进 durable projection watermark。

## Terminal

当 durable terminal event 发送完成：

- stream clean close；
- Host 清 watch task；
- Renderer 清 transient live state；
- LiveBus 可删除该 run 的 ring state。
