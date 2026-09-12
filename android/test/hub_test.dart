import 'dart:convert';

import 'package:agentpad/hub.dart';
import 'package:agentpad/store.dart';
import 'package:flutter_test/flutter_test.dart';

class RecordingLink extends PcLink {
  RecordingLink(super.hub, super.device, super.key, {required this.native});

  final bool native;
  final packets = <Map<String, dynamic>>[];

  @override
  bool sendPointer(
    double dx,
    double dy,
    int buttons,
    int wheel, {
    bool immediate = false,
  }) {
    if (!native) return false;
    packets.add({'dx': dx, 'dy': dy, 'buttons': buttons, 'wheel': wheel});
    return true;
  }

  @override
  void sendFast(String json) =>
      packets.add(jsonDecode(json) as Map<String, dynamic>);
}

void main() {
  test(
    'mixed transports scale each online target once and retain wheel fractions',
    () {
      final store = PadStore()
        ..devices = [
          Device(
            deviceId: 'mac',
            name: 'Mac',
            ips: ['127.0.0.1'],
            port: 9618,
            os: 'macos',
          ),
          Device(
            deviceId: 'win',
            name: 'Windows',
            ips: ['127.0.0.2'],
            port: 9618,
            os: 'windows',
          ),
        ];
      final hub = Hub(store);
      final mac = RecordingLink(hub, store.devices[0], 'mac', native: true);
      final win = RecordingLink(hub, store.devices[1], 'win', native: false);
      hub.links.addAll({'mac': mac, 'win': win});
      hub.online.addAll(['mac', 'win']);

      hub.sendPointer(1, 2, 0, 1);
      expect(mac.packets.last['wheel'], 16);
      expect(win.packets.last['wheel'], 1);
      expect(mac.packets.last['dx'], 3);
      expect(win.packets.last['dy'], 6);

      for (var i = 0; i < 2; i++) {
        hub.sendPointer(0, 0, 0, 0.5);
        expect(mac.packets.last['wheel'], 8);
        expect(win.packets.last['wheel'], i);
      }
      expect(mac.packets.length, 3);
      expect(win.packets.length, 3);

      store.wheelReverseMac = true;
      hub.sendPointer(0, 0, 0, 0.25);
      expect(mac.packets.last['wheel'], -4);
      expect(win.packets.last['wheel'], 0);
      store.devices[1].selected = false;
      hub.sendPointer(0, 0, 0, 0.5);
      expect(win.packets.length, 4);
      store.devices[1].selected = true;
      hub.online.remove('win');
      hub.sendPointer(0, 0, 0, 0.5);
      expect(win.packets.length, 4);
      hub.online.add('win');
      hub.sendPointer(0, 0, 0, 0.75);
      expect(win.packets.last['wheel'], 1);

      hub.sendPointer(0, 0, 1, 0.75, immediate: true);
      win.stop();
      hub.online.add('win');
      hub.sendPointer(0, 0, 0, 0.25, immediate: true);
      expect(win.packets.last['wheel'], 0);
      expect(win.packets.reversed.take(2).map((p) => p['buttons']), [0, 1]);
    },
  );
}
