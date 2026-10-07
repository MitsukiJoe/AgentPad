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
      "windows": {"name":"agentspads-windows-x64.exe","url":"https://example.com/a.exe","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
      "macos": {"name":"agentspads-macos-arm64.zip","url":"https://example.com/a.zip","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
      "android": {"name":"agentspads.apk","url":"https://github.com/MitsukiJoe/AgentsPads/releases/download/v1.2.3/agentspads.apk","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}
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
      'https://github.com/MitsukiJoe/AgentsPads/releases/download/v1.2.3/agentspads.apk',
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

  test('rejects versions that could escape the release path', () {
    const versions = [
      '9.9.9/%2e%2e/%2e%2e/%2e%2e/%2e%2e/%2e%2e/Evil/Repo/releases/download/v9.9.9',
      '9.9.9/../../../../../Evil/Repo/releases/download/v9.9.9',
    ];
    for (final bad in versions) {
      final escaped = validManifest
          .replaceAll('v1.2.3', 'v$bad')
          .replaceFirst('"version": "1.2.3"', '"version": "$bad"');
      expect(escaped, contains('releases/download/v$bad/agentspads.apk'));
      expect(parseAndroidUpdateManifest(escaped), isNull, reason: bad);
      expect(isNewerAppVersion(bad, '0.1.0'), isFalse, reason: bad);
    }
    for (final bad in ['9/', '9.9', '9.9.9.9', '9.9.9-beta', '9..9', '']) {
      expect(parseAppVersion(bad), isNull, reason: bad);
      expect(isNewerAppVersion(bad, '0.1.0'), isFalse, reason: bad);
    }
    expect(isNewerAppVersion('0.1.10', '0.1.9'), isTrue);
    expect(isNewerAppVersion('0.1.9', '0.1.9'), isFalse);
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

  test('debug build reports debug and has no comparable version', () {
    expect(appVersion, 'debug');
    expect(parseAppVersion(appVersion), isNull);
  });

  test('scan zoom maps real ratios onto CameraX linear zoom', () {
    expect(linearZoomFor(1, 1, 10), 0);
    expect(linearZoomFor(10, 1, 10), 1);
    expect(linearZoomFor(2, 1, 10), closeTo(5 / 9, 1e-9));
    expect(linearZoomFor(0.6, 0.6, 10), 0);
    expect(linearZoomFor(1, 1, 1), 0);
    expect(zoomPresets(0.6, 10), [0.6, 1, 2, 3, 5, 10]);
    expect(zoomPresets(1, 8), [1, 2, 3, 5]);
    expect(zoomPresets(1, 1), [1]);
    expect(zoomLabel(0.6), '0.6x');
    expect(zoomLabel(2), '2x');
  });
}
