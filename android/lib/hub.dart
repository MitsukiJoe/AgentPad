import 'dart:async';
import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import 'protocol.dart';
import 'store.dart';

typedef OnHub = void Function();

class Hub {
  Hub(this.store, {this.onChange});

  final PadStore store;
  final OnHub? onChange;
  final Map<String, PcLink> links = {};
  final Set<String> online = {};

  static String keyOf(Device d) => d.deviceId.isNotEmpty
      ? d.deviceId
      : (d.ips.isEmpty ? d.name : '${d.ips.first}:${d.port}');

  void sync() {
    final keys = {for (final d in store.devices) keyOf(d)};
    for (final k in links.keys.toList()) {
      if (!keys.contains(k)) {
        links.remove(k)?.stop();
        online.remove(k);
      }
    }
    for (final d in store.devices) {
      final k = keyOf(d);
      links.putIfAbsent(k, () => PcLink(this, d, k)..start());
      links[k]!.device = d;
    }
    onChange?.call();
  }

  Future<bool> sendTo(Device d, String json) async {
    for (final e in links.entries) {
      if (!online.contains(e.key)) continue;
      if (!_samePc(d, e.value.device)) continue;
      return e.value.send(json);
    }
    return false;
  }

  bool isOnline(Device d) => links.entries.any(
    (e) => online.contains(e.key) && _samePc(d, e.value.device),
  );

  static bool _samePc(Device a, Device b) {
    if (identical(a, b)) return true;
    if (a.deviceId.isNotEmpty && b.deviceId.isNotEmpty) {
      return a.deviceId == b.deviceId;
    }
    return a.ips.toSet().intersection(b.ips.toSet()).isNotEmpty;
  }

  Future<bool> sendSelected(String json) async {
    var any = false;
    for (final d in store.devices) {
      if (!d.selected) continue;
      if (await sendTo(d, json)) any = true;
    }
    return any;
  }

  void sendSelectedFast(String json) {
    for (final d in store.devices) {
      if (!d.selected) continue;
      sendToFast(d, json);
    }
  }

  /// Returns true when every selected online target consumed the native pump.
  bool sendPointer(
    double dx,
    double dy,
    int buttons,
    int wheel, {
    bool immediate = false,
  }) {
    var sent = false;
    var missed = false;
    for (final d in store.devices) {
      if (!d.selected) continue;
      for (final e in links.entries) {
        if (!online.contains(e.key)) continue;
        if (!_samePc(d, e.value.device)) continue;
        if (e.value.sendPointer(dx, dy, buttons, wheel, immediate: immediate)) {
          sent = true;
        } else {
          missed = true;
        }
        break;
      }
    }
    return sent && !missed;
  }

  void sendToFast(Device d, String json) {
    for (final e in links.entries) {
      if (!online.contains(e.key)) continue;
      if (!_samePc(d, e.value.device)) continue;
      if (e.value.hasPointerPump) return;
      e.value.sendFast(json);
      return;
    }
  }

  void dispose() {
    for (final l in links.values) {
      l.stop();
    }
    links.clear();
  }
}

class PcLink {
  PcLink(this.hub, this.device, this.key);

  final Hub hub;
  final String key;
  Device device;
  bool _stop = false;
  Future<bool> Function(String)? _send;
  void Function(String)? _sendFast;
  void Function(double, double, int, int, bool)? _sendPointer;
  Future<void> Function()? _close;

  void start() {
    _stop = false;
    () async {
      while (!_stop) {
        var connected = false;
        for (final ip in device.ips) {
          if (_stop) return;
          connected = await _try(ip, device.port);
          if (connected) break;
        }
        if (!connected) {
          await Future<void>.delayed(const Duration(seconds: 2));
        }
      }
    }();
  }

  void stop() {
    _stop = true;
    final c = _close;
    _close = null;
    _send = null;
    _sendFast = null;
    _sendPointer = null;
    hub.online.remove(key);
    c?.call();
  }

  bool get hasPointerPump => _sendPointer != null;

  Future<bool> send(String json) async {
    final s = _send;
    if (s == null) return false;
    return s(json);
  }

  void sendFast(String json) {
    final f = _sendFast;
    if (f != null) {
      f(json);
      return;
    }
    unawaited(send(json));
  }

  bool sendPointer(
    double dx,
    double dy,
    int buttons,
    int wheel, {
    bool immediate = false,
  }) {
    final f = _sendPointer;
    if (f == null) return false;
    f(dx, dy, buttons, wheel, immediate);
    return true;
  }

  Future<bool> _try(String host, int port) async {
    final id = key;
    var session = false;
    try {
      try {
        final native = await NativeWs.connect(
          id,
          host,
          port,
          onText: _onServer,
        );
        if (_stop) {
          await native?.close();
          return false;
        }
        if (native != null) {
          _send = native.send;
          _sendFast = native.sendFast;
          _sendPointer = native.addPointer;
          _close = native.close;
          hub.online.add(id);
          hub.onChange?.call();
          await native.send(helloMsg(hub.store.clientId, 'Android'));
          session = true;
          await native.done;
          return true;
        }
      } catch (_) {}
      if (_stop) return false;
      final ch = WebSocketChannel.connect(Uri.parse('ws://$host:$port'));
      await ch.ready.timeout(const Duration(seconds: 4));
      if (_stop) {
        await ch.sink.close();
        return false;
      }
      _send = (String j) async {
        ch.sink.add(j);
        return true;
      };
      _sendFast = (String j) => ch.sink.add(j);
      _close = () async => ch.sink.close();
      hub.online.add(id);
      hub.onChange?.call();
      ch.sink.add(helloMsg(hub.store.clientId, 'Android'));
      session = true;
      await for (final msg in ch.stream) {
        if (msg is String) _onServer(msg);
      }
      return true;
    } catch (_) {
      return session;
    } finally {
      hub.online.remove(id);
      _send = null;
      _sendFast = null;
      _sendPointer = null;
      _close = null;
      hub.onChange?.call();
    }
  }

  void _onServer(String raw) {
    try {
      final v = jsonDecode(raw);
      if (v is Map && v['type'] == 'connected') {
        final did = v['device_id'] as String? ?? '';
        if (did.isNotEmpty) device.deviceId = did;
        // Display name is user-owned; never overwrite from the PC hostname.
        if (hub.store.collectAllIps && v['ips'] is List) {
          for (final e in v['ips'] as List) {
            final ip = e.toString();
            if (ip.isNotEmpty && !device.ips.contains(ip)) device.ips.add(ip);
          }
        }
        hub.store.save();
        hub.onChange?.call();
      }
    } catch (_) {}
  }
}

class NativeWs {
  NativeWs._(this.id, this._done);
  final String id;
  final Completer<void> _done;

  Future<void> get done => _done.future;

  static const _m = MethodChannel('agentpad/ws');
  static const _e = EventChannel('agentpad/ws_events');
  static Stream<dynamic>? _events;
  static StreamSubscription<dynamic>? _sub;
  static final _waiters = <String, Completer<void>>{};
  static final _texts = <String, void Function(String)>{};

  static Future<bool> voiceEvidence() async {
    try {
      return await _m.invokeMethod<bool>('voiceEvidence') ?? false;
    } catch (_) {
      return false;
    }
  }

  static Future<void> resetVoiceEvidence() async {
    try {
      await _m.invokeMethod<void>('resetVoiceEvidence');
    } catch (_) {}
  }

  static void _listen() {
    _events ??= _e.receiveBroadcastStream();
    _sub ??= _events!.listen((ev) {
      if (ev is! Map) return;
      final id = ev['id'] as String?;
      if (id == null) return;
      if (ev['event'] == 'close') {
        _texts.remove(id);
        _waiters.remove(id)?.complete();
      } else if (ev['event'] == 'text') {
        final data = ev['data'] as String?;
        if (data != null) _texts[id]?.call(data);
      }
    });
  }

  static Future<NativeWs?> connect(
    String id,
    String host,
    int port, {
    void Function(String)? onText,
  }) async {
    _listen();
    if (onText != null) _texts[id] = onText;
    final done = Completer<void>();
    _waiters[id] = done;
    try {
      final ok = await _m.invokeMethod<bool>('connect', {
        'id': id,
        'host': host,
        'port': port,
      });
      if (ok != true) {
        _forget(id, done);
        return null;
      }
    } on MissingPluginException {
      _forget(id, done);
      return null;
    } on PlatformException {
      _forget(id, done);
      return null;
    }
    return NativeWs._(id, done);
  }

  static void _forget(String id, Completer<void> done) {
    if (!identical(_waiters[id], done)) return;
    _texts.remove(id);
    _waiters.remove(id);
  }

  static Future<double?> displayRefreshHz() async {
    try {
      final v = await _m.invokeMethod<num>('displayRefreshHz');
      return v?.toDouble();
    } catch (_) {
      return null;
    }
  }

  Future<bool> send(String text) async {
    try {
      return await _m.invokeMethod<bool>('send', {'id': id, 'text': text}) ==
          true;
    } catch (_) {
      return false;
    }
  }

  void sendFast(String text) {
    unawaited(send(text));
  }

  void addPointer(
    double dx,
    double dy,
    int buttons,
    int wheel,
    bool immediate,
  ) {
    unawaited(
      _m.invokeMethod('pointer', {
        'id': id,
        'dx': dx,
        'dy': dy,
        'buttons': buttons,
        'wheel': wheel,
        'immediate': immediate,
      }),
    );
  }

  Future<void> close() async {
    if (identical(_waiters[id], _done)) {
      _texts.remove(id);
      _waiters.remove(id);
      _done.complete();
    }
    try {
      await _m.invokeMethod('close', {'id': id});
    } catch (_) {}
  }
}

bool sendClearsInput(bool anySendSucceeded) => anySendSucceeded;
