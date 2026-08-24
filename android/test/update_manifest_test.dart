import 'package:flutter_test/flutter_test.dart';

import 'package:agentpad/app.dart';

void main() {
  const validManifest = '''
  {
    "schema": 1,
    "tag_name": "v1.2.3",
    "version": "1.2.3",
    "release_url": "https://example.com/v1.2.3",
    "body": "notes",
    "assets": {
      "windows": {"name":"agentpad-windows-x64.exe","url":"https://example.com/a.exe","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
      "macos": {"name":"agentpad-macos-arm64.zip","url":"https://example.com/a.zip","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
      "android": {"name":"agentpad.apk","url":"https://github.com/MitsukiJoe/AgentPad/releases/download/v1.2.3/agentpad.apk","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}
    }
  }
  ''';

  test('parses Android asset from update manifest', () {
    final update = parseAndroidUpdateManifest(validManifest);
    expect(update, isNotNull);
    expect(update!.version, '1.2.3');
    expect(update.tagName, 'v1.2.3');
    expect(
      update.apkUrl,
      'https://github.com/MitsukiJoe/AgentPad/releases/download/v1.2.3/agentpad.apk',
    );
    expect(update.sha256, hasLength(64));
  });

  test('rejects malformed update manifest hash', () {
    final malformed = validManifest.replaceFirst(
      'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
      'bad-hash',
    );
    expect(parseAndroidUpdateManifest(malformed), isNull);
  });

  test('selects the newest valid CDN manifest', () {
    const older = AndroidUpdateInfo(
      tagName: 'v1.2.3',
      version: '1.2.3',
      body: 'older',
      apkUrl: 'https://example.com/older.apk',
      sha256: 'a',
    );
    const newer = AndroidUpdateInfo(
      tagName: 'v1.2.4',
      version: '1.2.4',
      body: 'newer',
      apkUrl: 'https://example.com/newer.apk',
      sha256: 'b',
    );
    expect(newerAndroidUpdate(null, older), same(older));
    expect(newerAndroidUpdate(older, newer), same(newer));
    expect(newerAndroidUpdate(newer, older), same(newer));
  });

  test('uses GitHub, jsDelivr, then JSDMirror manifest sources', () {
    final urls = androidUpdateManifestUris(DateTime.utc(2026, 1, 1));
    expect(urls, hasLength(3));
    expect(urls[0].host, 'github.com');
    expect(urls[1].host, 'cdn.jsdelivr.net');
    expect(urls[2].host, 'cdn.jsdmirror.com');
    expect(urls[1].queryParameters['hour'], isNotEmpty);
    expect(urls[2].queryParameters['hour'], urls[1].queryParameters['hour']);
  });
}
