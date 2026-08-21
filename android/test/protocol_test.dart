import 'package:flutter_test/flutter_test.dart';

import 'package:agentpad/protocol.dart';

void main() {
  test('qr payload required fields', () {
    const raw =
        '{"v":1,"type":"agentpad","device_id":"d1","ip":"192.168.1.2","port":9618,"name":"Mac","os":"macos","ips":["192.168.1.2","10.0.0.5"]}';
    final q = QrPayload.parse(raw)!;
    expect(q.deviceId, 'd1');
    expect(q.ip, '192.168.1.2');
    expect(q.port, 9618);
    expect(q.ips, ['192.168.1.2', '10.0.0.5']);
  });

  test('qr parse puts selected ip first', () {
    final q = QrPayload.parse(
      '{"v":1,"type":"agentpad","device_id":"d","ip":"10.0.0.5","port":9618,"name":"M","os":"macos","ips":["192.168.1.2","10.0.0.5"]}',
    )!;
    expect(q.ips, ['10.0.0.5', '192.168.1.2']);
  });

  test('qr rejects other types', () {
    expect(QrPayload.parse('{"v":1,"type":"other","ip":"1.1.1.1"}'), isNull);
    expect(QrPayload.parse('not json'), isNull);
    expect(
      QrPayload.parse(
        '  {"v":"1","type":"agentpad","device_id":"d","ip":"10.0.0.1","port":9618,"name":"M","os":"macos"}  ',
      )?.ip,
      '10.0.0.1',
    );
  });

  test('parse host port', () {
    expect(parseHostPort('10.0.0.5').port, 9618);
    expect(parseHostPort('10.0.0.5:9618').host, '10.0.0.5');
    expect(parseHostPort('10.0.0.5:1234').port, 1234);
  });

  test('text submit json', () {
    expect(
      textMsg('hi', autoEnter: true, sendMode: 'submit'),
      contains('"send_mode":"submit"'),
    );
    expect(keyMsg('Escape', []), contains('"key":"Escape"'));
  });
}
