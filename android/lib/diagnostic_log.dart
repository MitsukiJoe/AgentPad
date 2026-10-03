import 'dart:async';
import 'dart:convert';

import 'package:flutter/services.dart';

enum DiagnosticKind {
  text,
  key,
  pointer,
  undo,
  ping,
  hello,
  connected,
  unknown,
  connection,
  lifecycle,
  update,
}

enum DiagnosticStage { send, receive, start, stop, check }

enum DiagnosticResult { ok, failed, active, inactive }

class DiagnosticLog {
  static const _channel = MethodChannel('agentpad/ws');
  static bool enabled = false;
  static Timer? _pointerTimer;
  static int _pointerGeneration = 0;
  static final _pointerCounts = <String, int>{};

  static Future<bool> refresh() async {
    try {
      enabled = await _channel.invokeMethod<bool>('diagnosticEnabled') ?? false;
    } catch (_) {
      enabled = false;
    }
    return enabled;
  }

  static Future<void> setEnabled(bool value) async {
    _pointerGeneration++;
    _pointerTimer?.cancel();
    _pointerTimer = null;
    _pointerCounts.clear();
    try {
      enabled =
          await _channel.invokeMethod<bool>('diagnosticSetEnabled', value) ??
          false;
    } catch (_) {
      enabled = false;
    }
  }

  static DiagnosticKind classify(String raw) {
    try {
      final value = jsonDecode(raw);
      if (value is Map) {
        return switch (value['type']) {
          'text' => DiagnosticKind.text,
          'key' => DiagnosticKind.key,
          'pointer' => DiagnosticKind.pointer,
          'undo' => DiagnosticKind.undo,
          'ping' => DiagnosticKind.ping,
          'hello' => DiagnosticKind.hello,
          'connected' => DiagnosticKind.connected,
          _ => DiagnosticKind.unknown,
        };
      }
    } catch (_) {}
    return DiagnosticKind.unknown;
  }

  static Future<void> record(
    DiagnosticKind kind,
    DiagnosticStage stage,
    DiagnosticResult result,
  ) async {
    if (!enabled) return;
    if (kind == DiagnosticKind.pointer) {
      final key = '${stage.name}:${result.name}';
      _pointerCounts[key] = (_pointerCounts[key] ?? 0) + 1;
      final generation = _pointerGeneration;
      _pointerTimer ??= Timer(const Duration(seconds: 1), () async {
        final counts = Map<String, int>.of(_pointerCounts);
        _pointerCounts.clear();
        _pointerTimer = null;
        if (!enabled) return;
        for (final entry in counts.entries) {
          if (!enabled || generation != _pointerGeneration) return;
          final parts = entry.key.split(':');
          try {
            await _channel.invokeMethod<void>('diagnosticEvent', {
              'kind': 'pointer',
              'stage': parts[0],
              'result': parts[1],
              'count': entry.value,
            });
          } catch (_) {}
        }
      });
      return;
    }
    try {
      await _channel.invokeMethod<void>('diagnosticEvent', {
        'kind': kind.name,
        'stage': stage.name,
        'result': result.name,
      });
    } catch (_) {}
  }

  static Future<String> read() async {
    try {
      return await _channel.invokeMethod<String>('diagnosticRead') ?? '';
    } catch (_) {
      return '读取失败';
    }
  }

  static Future<bool> clear() async {
    _pointerGeneration++;
    _pointerTimer?.cancel();
    _pointerTimer = null;
    _pointerCounts.clear();
    try {
      return await _channel.invokeMethod<bool>('diagnosticClear') ?? false;
    } catch (_) {
      return false;
    }
  }
}
