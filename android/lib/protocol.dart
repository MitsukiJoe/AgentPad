import 'dart:convert';

const int kPort = 9618;

class QrPayload {
  QrPayload({
    required this.deviceId,
    required this.ip,
    required this.port,
    required this.name,
    required this.os,
    required this.ips,
  });

  final String deviceId;
  final String ip;
  final int port;
  final String name;
  final String os;
  final List<String> ips;

  static QrPayload? parse(String raw) {
    late final Object decoded;
    try {
      decoded = jsonDecode(raw.trim());
    } catch (_) {
      return null;
    }
    if (decoded is! Map) return null;
    final map = decoded.cast<String, dynamic>();
    if (!_isV1(map['v']) || map['type']?.toString() != 'agentpad') return null;
    final ip = map['ip'] as String? ?? '';
    if (ip.isEmpty) return null;
    final ips = <String>[
      if (map['ips'] is List)
        for (final e in map['ips'] as List) e.toString(),
    ];
    ips.remove(ip);
    ips.insert(0, ip);
    return QrPayload(
      deviceId: map['device_id'] as String? ?? '',
      ip: ip,
      port: (map['port'] as num?)?.toInt() ?? kPort,
      name: map['name'] as String? ?? ip,
      os: map['os'] as String? ?? '',
      ips: ips,
    );
  }
}

String textMsg(
  String content, {
  required bool autoEnter,
  required String sendMode,
}) => jsonEncode({
  'type': 'text',
  'content': content,
  'auto_enter': autoEnter,
  'send_mode': sendMode,
});

String keyMsg(String key, List<String> modifiers) =>
    jsonEncode({'type': 'key', 'key': key, 'modifiers': modifiers});

String pointerMsg(double dx, double dy, int buttons, int wheel) => jsonEncode({
  'type': 'pointer',
  'dx': dx,
  'dy': dy,
  'buttons': buttons,
  'wheel': wheel,
});

String undoMsg() => jsonEncode({'type': 'undo'});

String pingMsg() => jsonEncode({'type': 'ping'});

String helloMsg(String clientId, String clientName) => jsonEncode({
  'type': 'hello',
  'client_id': clientId,
  'client_name': clientName,
});

HostPort parseHostPort(String raw) {
  final s = raw.trim();
  final i = s.lastIndexOf(':');
  if (i > 0 && i < s.length - 1) {
    final port = int.tryParse(s.substring(i + 1));
    if (port != null) {
      return HostPort(s.substring(0, i), port);
    }
  }
  return HostPort(s, kPort);
}

class HostPort {
  HostPort(this.host, this.port);
  final String host;
  final int port;
}

bool _isV1(dynamic v) {
  if (v == 1 || v == '1') return true;
  if (v is num) return v.toInt() == 1;
  return false;
}
