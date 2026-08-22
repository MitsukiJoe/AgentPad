import 'dart:convert';
import 'dart:math';

import 'package:shared_preferences/shared_preferences.dart';

import 'protocol.dart';

class Device {
  Device({
    required this.deviceId,
    required this.name,
    required this.ips,
    required this.port,
    this.selected = true,
  });

  String deviceId;
  String name;
  List<String> ips;
  int port;
  bool selected;

  Map<String, dynamic> toJson() => {
    'device_id': deviceId,
    'name': name,
    'ips': ips,
    'port': port,
    'selected': selected,
  };

  static Device fromJson(Map<String, dynamic> j) => Device(
    deviceId: j['device_id'] as String? ?? '',
    name: j['name'] as String? ?? '',
    ips: [
      if (j['ips'] is List)
        for (final e in j['ips'] as List) e.toString(),
    ],
    port: (j['port'] as num?)?.toInt() ?? kPort,
    selected: j['selected'] as bool? ?? true,
  );

  /// Merge IPs / id / port. Display [name] stays unless this device has none.
  Device merge(Device other) {
    final union = [...ips];
    for (final ip in other.ips) {
      if (!union.contains(ip)) union.add(ip);
    }
    return Device(
      deviceId: other.deviceId.isNotEmpty ? other.deviceId : deviceId,
      name: name.isNotEmpty ? name : other.name,
      ips: union,
      port: other.port,
      selected: selected,
    );
  }
}

List<Device> upsertDevice(List<Device> list, Device incoming) {
  if (incoming.deviceId.isNotEmpty) {
    final byId = list.indexWhere((d) => d.deviceId == incoming.deviceId);
    if (byId >= 0) {
      final next = [...list];
      next[byId] = next[byId].merge(incoming);
      return next;
    }
  }
  final incomingIps = incoming.ips.toSet();
  final byIp = list.indexWhere((d) {
    if (d.ips.toSet().intersection(incomingIps).isEmpty) return false;
    // Same LAN IP on different machines must stay separate.
    if (incoming.deviceId.isNotEmpty &&
        d.deviceId.isNotEmpty &&
        incoming.deviceId != d.deviceId) {
      return false;
    }
    return true;
  });
  if (byIp >= 0) {
    final next = [...list];
    next[byIp] = next[byIp].merge(incoming);
    return next;
  }
  return [...list, incoming];
}

class Shortcut {
  Shortcut({
    required this.id,
    required this.label,
    required this.key,
    required this.modifiers,
  });

  final String id;
  String label;
  String key;
  List<String> modifiers;

  Map<String, dynamic> toJson() => {
    'id': id,
    'label': label,
    'key': key,
    'modifiers': modifiers,
  };

  static Shortcut fromJson(Map<String, dynamic> j) => Shortcut(
    id: j['id'] as String? ?? '',
    label: j['label'] as String? ?? '',
    key: j['key'] as String? ?? 'Escape',
    modifiers: [
      if (j['modifiers'] is List)
        for (final e in j['modifiers'] as List) e.toString(),
    ],
  );
}

List<Shortcut> defaultShortcuts() => [
  Shortcut(id: 'esc', label: 'Esc', key: 'Escape', modifiers: []),
  Shortcut(id: 'enter', label: 'Enter', key: 'Enter', modifiers: []),
  Shortcut(
    id: 'senter',
    label: 'Shift+Enter',
    key: 'Enter',
    modifiers: ['Shift'],
  ),
];

class PadStore {
  List<Device> devices = [];
  List<Shortcut> shortcuts = defaultShortcuts();
  bool autoEnter = false;
  bool voiceAutoSend = true;
  /// After connect, merge all LAN IPs from the PC into this device (off by default).
  bool collectAllIps = false;
  int voiceDelayMs = 500;
  String pointerMode = 'trackpad';
  bool homePointerQuickSwitch = true;
  String wheelSide = 'right';
  String pointerSize = 'medium';
  int pointerHz = 60;
  var pointerHzManual = false;
  double pointerSpeed = 2;
  double wheelSpeed = 16;
  String inputHeight = 'medium';
  String landscapePointerSide = 'left';
  String clientId = '';
  String theme = 'system';
  String themeColor = 'blue';
  String appIcon = 'system';

  Duration get voiceDelay => Duration(milliseconds: voiceDelayMs);

  static const pointerGears = [1.0, 2.0, 3.0, 4.0];
  // Actual scroll multipliers (includes the stronger wheel base vs pointer).
  static const wheelGears = [4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0];

  static double coerceGear(double raw, List<double> gears, double fallback) {
    for (final g in gears) {
      if (g == raw) return g;
    }
    return fallback;
  }

  static double nearestGear(double raw, List<double> gears, double fallback) {
    var best = fallback;
    var bestDist = double.infinity;
    for (final g in gears) {
      final d = (g - raw).abs();
      if (d < bestDist) {
        bestDist = d;
        best = g;
      }
    }
    return best;
  }

  Future<void> load() async {
    final p = await SharedPreferences.getInstance();
    clientId = p.getString('client_id') ?? '';
    if (clientId.isEmpty) {
      clientId =
          '${DateTime.now().microsecondsSinceEpoch}-${Random().nextInt(1 << 32)}';
      await p.setString('client_id', clientId);
    }
    autoEnter = p.getBool('auto_enter') ?? false;
    voiceAutoSend = p.getBool('voice_auto_send') ?? true;
    collectAllIps = p.getBool('collect_all_ips') ?? false;
    voiceDelayMs = p.getInt('voice_delay_ms') ?? 500;
    if (!{0, 500, 1000, 1500}.contains(voiceDelayMs)) voiceDelayMs = 500;
    pointerMode =
        p.getString('pointer_mode') ??
        ((p.getBool('trackpoint') ?? false) ? 'trackpoint' : 'trackpad');
    if (!{'trackpad', 'trackball', 'trackpoint'}.contains(pointerMode)) {
      pointerMode = 'trackpad';
    }
    homePointerQuickSwitch = p.getBool('home_pointer_quick_switch') ?? true;
    wheelSide = p.getString('wheel_side') ?? 'right';
    if (!{'left', 'right'}.contains(wheelSide)) wheelSide = 'right';
    pointerSize = p.getString('pointer_size') ?? 'medium';
    if (!{'small', 'medium', 'large'}.contains(pointerSize)) {
      pointerSize = 'medium';
    }
    pointerHzManual = p.getBool('pointer_hz_manual') ?? false;
    pointerHz = p.getInt('pointer_hz') ?? 60;
    if (!{60, 120, 240}.contains(pointerHz)) pointerHz = 60;
    if (!pointerHzManual && pointerHz == 240) pointerHz = 60;
    pointerSpeed = coerceGear(
      (p.getDouble('pointer_speed') ?? 2).roundToDouble(),
      pointerGears,
      2,
    );
    if (p.containsKey('wheel_factor')) {
      wheelSpeed = nearestGear(p.getDouble('wheel_factor') ?? 16, wheelGears, 16);
    } else {
      final legacy = p.getDouble('wheel_speed');
      // Old 1–9 gear index had a hidden ×4 gain.
      wheelSpeed = legacy == null
          ? 16
          : nearestGear(legacy * 4, wheelGears, 16);
    }
    inputHeight = p.getString('input_height') ?? 'medium';
    if (inputHeight == 'short') inputHeight = 'medium';
    if (!{'medium', 'tall', 'huge'}.contains(inputHeight)) {
      inputHeight = 'medium';
    }
    landscapePointerSide = p.getString('landscape_pointer_side') ?? 'left';
    if (!{'left', 'right'}.contains(landscapePointerSide)) {
      landscapePointerSide = 'left';
    }
    theme = p.getString('theme') ?? 'system';
    appIcon = p.getString('app_icon') ?? 'system';
    if (!{'white', 'black', 'system'}.contains(appIcon)) {
      appIcon = 'system';
    }
    themeColor = p.getString('theme_color') ?? 'blue';
    if (!{
      'blue',
      'monochrome',
      'green',
      'pink',
      'gold',
      'red',
    }.contains(themeColor)) {
      themeColor = 'blue';
    }
    final rawDev = p.getString('devices');
    if (rawDev != null) {
      final list = jsonDecode(rawDev) as List;
      devices = [
        for (final e in list)
          Device.fromJson((e as Map).cast<String, dynamic>()),
      ];
    }
    final rawKeys = p.getString('shortcuts');
    if (rawKeys != null) {
      final list = jsonDecode(rawKeys) as List;
      shortcuts = [
        for (final e in list)
          Shortcut.fromJson((e as Map).cast<String, dynamic>()),
      ];
    }
  }

  Future<void> save() async {
    final p = await SharedPreferences.getInstance();
    await p.setBool('auto_enter', autoEnter);
    await p.setBool('voice_auto_send', voiceAutoSend);
    await p.setBool('collect_all_ips', collectAllIps);
    await p.setInt('voice_delay_ms', voiceDelayMs);
    await p.setString('pointer_mode', pointerMode);
    await p.setBool('home_pointer_quick_switch', homePointerQuickSwitch);
    await p.setString('wheel_side', wheelSide);
    await p.setString('pointer_size', pointerSize);
    await p.setInt('pointer_hz', pointerHz);
    await p.setBool('pointer_hz_manual', pointerHzManual);
    await p.setDouble('pointer_speed', pointerSpeed);
    await p.setDouble('wheel_factor', wheelSpeed);
    await p.setString('input_height', inputHeight);
    await p.setString('landscape_pointer_side', landscapePointerSide);
    await p.setString('theme', theme);
    await p.setString('theme_color', themeColor);
    await p.setString('app_icon', appIcon);
    await p.setString(
      'devices',
      jsonEncode([for (final d in devices) d.toJson()]),
    );
    await p.setString(
      'shortcuts',
      jsonEncode([for (final s in shortcuts) s.toJson()]),
    );
  }
}
