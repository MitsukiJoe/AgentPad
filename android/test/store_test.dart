import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:agentpad/hub.dart';
import 'package:agentpad/store.dart';

void main() {
  test('upsert merges by device_id and keeps others', () {
    final a = Device(deviceId: 'a', name: 'A', ips: ['1.1.1.1'], port: 9618);
    final b = Device(deviceId: 'b', name: 'B', ips: ['2.2.2.2'], port: 9618);
    var list = upsertDevice([], a);
    list = upsertDevice(list, b);
    list = upsertDevice(
      list,
      Device(
        deviceId: 'a',
        name: 'A2',
        ips: ['1.1.1.1', '3.3.3.3'],
        port: 9618,
      ),
    );
    expect(list.length, 2);
    expect(list[0].name, 'A'); // display name stays
    expect(list[0].ips, ['1.1.1.1', '3.3.3.3']);
    expect(list[1].deviceId, 'b');
  });

  test('same ip different device_id stays separate', () {
    final list = upsertDevice([
      Device(deviceId: 'a', name: 'A', ips: ['10.0.0.5'], port: 9618),
    ], Device(deviceId: 'b', name: 'B', ips: ['10.0.0.5'], port: 9618));
    expect(list.length, 2);
    expect(list[0].name, 'A');
    expect(list[1].name, 'B');
  });

  test('hub keeps different device ids separate when ips overlap', () {
    final hub = Hub(PadStore());
    final online = Device(
      deviceId: 'a',
      name: 'A',
      ips: ['10.0.0.5'],
      port: 9618,
    );
    hub.links['a'] = PcLink(hub, online, 'a');
    hub.online.add('a');

    final other = Device(
      deviceId: 'b',
      name: 'B',
      ips: ['10.0.0.5'],
      port: 9618,
    );
    expect(hub.isOnline(other), isFalse);
  });

  test('hub falls back to overlapping ip for a legacy device without id', () {
    final hub = Hub(PadStore());
    final online = Device(
      deviceId: 'a',
      name: 'A',
      ips: ['10.0.0.5'],
      port: 9618,
    );
    hub.links['a'] = PcLink(hub, online, 'a');
    hub.online.add('a');

    final legacy = Device(
      deviceId: '',
      name: 'legacy',
      ips: ['10.0.0.5'],
      port: 9618,
    );
    expect(hub.isOnline(legacy), isTrue);
  });

  test('upsert merges by overlapping ip when no device_id', () {
    final list = upsertDevice([
      Device(deviceId: '', name: 'old', ips: ['10.0.0.5'], port: 9618),
    ], Device(deviceId: 'x', name: 'new', ips: ['10.0.0.5'], port: 9618));
    expect(list.length, 1);
    expect(list.first.deviceId, 'x');
    expect(list.first.name, 'old');
  });

  test('collect all ips setting defaults off and persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.collectAllIps, isFalse);
    s.collectAllIps = true;
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.collectAllIps, isTrue);
  });

  test('send clears input only after a successful send', () {
    expect(sendClearsInput(true), isTrue);
    expect(sendClearsInput(false), isFalse);
  });

  test('shortcuts persist roundtrip', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    s.shortcuts.add(Shortcut(id: 'x', label: 'Tab', key: 'Tab', modifiers: []));
    s.autoEnter = true;
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.autoEnter, isTrue);
    expect(s2.shortcuts.any((k) => k.label == 'Tab'), isTrue);
    expect(s2.shortcuts.any((k) => k.label == 'Esc'), isTrue);
  });

  test('theme persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.theme, 'system');
    s.theme = 'dark';
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.theme, 'dark');
  });

  test('theme color defaults to blue, validates, and persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.themeColor, 'blue');
    s.themeColor = 'green';
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.themeColor, 'green');

    SharedPreferences.setMockInitialValues({'theme_color': 'purple'});
    final invalid = PadStore();
    await invalid.load();
    expect(invalid.themeColor, 'blue');
  });

  test('pointer mode and wheel side persist', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.pointerMode, 'trackpad');
    expect(s.homePointerQuickSwitch, isTrue);
    expect(s.wheelSide, 'right');
    s.pointerMode = 'trackball';
    s.homePointerQuickSwitch = false;
    s.wheelSide = 'left';
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.pointerMode, 'trackball');
    expect(s2.homePointerQuickSwitch, isFalse);
    expect(s2.wheelSide, 'left');
  });

  test('pointer hz defaults to 60 and persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.pointerHz, 60);
    expect(s.pointerHzManual, isFalse);
    s.pointerHz = 240;
    s.pointerHzManual = true;
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.pointerHz, 240);
    expect(s2.pointerHzManual, isTrue);

    SharedPreferences.setMockInitialValues({'pointer_hz': 90});
    final invalid = PadStore();
    await invalid.load();
    expect(invalid.pointerHz, 60);
  });

  test('pointer and wheel speed default and persist', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.pointerSpeed, 2);
    expect(s.wheelSpeed, 16);
    s.pointerSpeed = 4;
    s.wheelSpeed = 28;
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.pointerSpeed, 4);
    expect(s2.wheelSpeed, 28);

    SharedPreferences.setMockInitialValues({
      'pointer_speed': 5.0,
      'wheel_factor': 11.0,
    });
    final invalid = PadStore();
    await invalid.load();
    expect(invalid.pointerSpeed, 2);
    expect(invalid.wheelSpeed, 12);

    SharedPreferences.setMockInitialValues({'wheel_speed': 4.0});
    final legacy = PadStore();
    await legacy.load();
    expect(legacy.wheelSpeed, 16);
  });

  test('non-manual 240 falls back before auto detect', () async {
    SharedPreferences.setMockInitialValues({
      'pointer_hz': 240,
      'pointer_hz_manual': false,
    });
    final s = PadStore();
    await s.load();
    expect(s.pointerHzManual, isFalse);
    expect(s.pointerHz, 60);
  });

  test('pointer size persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.pointerSize, 'medium');
    s.pointerSize = 'large';
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.pointerSize, 'large');
  });

  test('input height defaults to medium and persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.inputHeight, 'medium');
    s.inputHeight = 'huge';
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.inputHeight, 'huge');

    SharedPreferences.setMockInitialValues({'input_height': 'short'});
    final legacy = PadStore();
    await legacy.load();
    expect(legacy.inputHeight, 'medium');

    SharedPreferences.setMockInitialValues({'input_height': 'huge'});
    final ok = PadStore();
    await ok.load();
    expect(ok.inputHeight, 'huge');
  });

  test('landscape pointer side defaults, validates, and persists', () async {
    SharedPreferences.setMockInitialValues({});
    final s = PadStore();
    await s.load();
    expect(s.landscapePointerSide, 'left');
    s.landscapePointerSide = 'right';
    await s.save();
    final s2 = PadStore();
    await s2.load();
    expect(s2.landscapePointerSide, 'right');

    SharedPreferences.setMockInitialValues({
      'landscape_pointer_side': 'bottom',
    });
    final invalid = PadStore();
    await invalid.load();
    expect(invalid.landscapePointerSide, 'left');
  });

  test(
    'voice auto-send delay defaults to half a second and persists',
    () async {
      SharedPreferences.setMockInitialValues({});
      final s = PadStore();
      await s.load();
      expect(s.voiceDelayMs, 500);
      expect(s.voiceDelay, const Duration(milliseconds: 500));
      s.voiceDelayMs = 1000;
      await s.save();
      final s2 = PadStore();
      await s2.load();
      expect(s2.voiceDelayMs, 1000);
      expect(s2.voiceDelay, const Duration(seconds: 1));
    },
  );

  test('invalid voice auto-send delay falls back to half a second', () async {
    SharedPreferences.setMockInitialValues({'voice_delay_ms': 750});
    final s = PadStore();
    await s.load();
    expect(s.voiceDelayMs, 500);
  });

  test('legacy trackpoint preference migrates to pointer mode', () async {
    SharedPreferences.setMockInitialValues({'trackpoint': true});
    final s = PadStore();
    await s.load();
    expect(s.pointerMode, 'trackpoint');
  });
}
