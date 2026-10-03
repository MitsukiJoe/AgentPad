import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:agentpad/app.dart';
import 'package:agentpad/store.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentpad/diagnostic_log.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('agentpad/ws');
  final calls = <MethodCall>[];
  var nativeEnabled = false;
  var log = '';

  setUp(() async {
    calls.clear();
    nativeEnabled = false;
    log = '';
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          switch (call.method) {
            case 'diagnosticSetEnabled':
              nativeEnabled = call.arguments == true;
              if (nativeEnabled) log = '';
              return nativeEnabled;
            case 'diagnosticEnabled':
              return nativeEnabled;
            case 'diagnosticRead':
              return log;
            case 'diagnosticClear':
              log = '';
              return true;
            case 'diagnosticEvent':
              if (nativeEnabled) log += '${call.arguments}';
              return null;
          }
          return null;
        });
    await DiagnosticLog.setEnabled(false);
    calls.clear();
  });

  tearDown(() async => DiagnosticLog.setEnabled(false));

  test(
    'off sends no event; enabled sends only whitelisted classification',
    () async {
      const raw =
          '{"type":"text","content":"SECRET 192.168.1.12 /private/key UUID"}';
      await DiagnosticLog.record(
        DiagnosticLog.classify(raw),
        DiagnosticStage.send,
        DiagnosticResult.ok,
      );
      expect(calls, isEmpty);
      await DiagnosticLog.setEnabled(true);
      await DiagnosticLog.record(
        DiagnosticLog.classify(raw),
        DiagnosticStage.send,
        DiagnosticResult.ok,
      );
      expect(calls.last.arguments, {
        'kind': 'text',
        'stage': 'send',
        'result': 'ok',
      });
      expect(await DiagnosticLog.read(), isNot(contains('SECRET')));
      expect(
        DiagnosticLog.classify('{"type":"SECRET"}'),
        DiagnosticKind.unknown,
      );
      expect(DiagnosticLog.classify('{broken SECRET'), DiagnosticKind.unknown);
      expect(
        DiagnosticLog.classify('{"type":"key","key":"SECRET"}'),
        DiagnosticKind.key,
      );
    },
  );

  test(
    'toggle and refresh use process state; view and clear use private native log',
    () async {
      await DiagnosticLog.setEnabled(true);
      await DiagnosticLog.record(
        DiagnosticKind.key,
        DiagnosticStage.send,
        DiagnosticResult.ok,
      );
      expect(await DiagnosticLog.read(), contains('key'));
      await DiagnosticLog.clear();
      expect(await DiagnosticLog.read(), isEmpty);
      await DiagnosticLog.setEnabled(false);
      calls.clear();
      await DiagnosticLog.record(
        DiagnosticKind.text,
        DiagnosticStage.send,
        DiagnosticResult.ok,
      );
      expect(calls, isEmpty);
      await DiagnosticLog.setEnabled(true);
      nativeEnabled = false;
      expect(await DiagnosticLog.refresh(), false);
      expect(DiagnosticLog.enabled, false);
      expect(calls.every((call) => call.method.startsWith('diagnostic')), true);
    },
  );

  testWidgets(
    'pointer values never cross diagnostic channel and aggregate once per second',
    (tester) async {
      await DiagnosticLog.setEnabled(true);
      for (var i = 0; i < 240; i++) {
        await DiagnosticLog.record(
          DiagnosticLog.classify(
            '{"type":"pointer","dx":12345,"buttons":3,"wheel":77}',
          ),
          DiagnosticStage.send,
          DiagnosticResult.ok,
        );
      }
      expect(calls.where((call) => call.method == 'diagnosticEvent'), isEmpty);
      await tester.pump(const Duration(seconds: 1));
      final events = calls
          .where((call) => call.method == 'diagnosticEvent')
          .toList();
      expect(events, hasLength(1));
      expect(events.single.arguments, {
        'kind': 'pointer',
        'stage': 'send',
        'result': 'ok',
        'count': 240,
      });
      await DiagnosticLog.record(
        DiagnosticKind.pointer,
        DiagnosticStage.send,
        DiagnosticResult.ok,
      );
      await DiagnosticLog.setEnabled(false);
      await tester.pump(const Duration(seconds: 1));
      expect(
        calls.where((call) => call.method == 'diagnosticEvent'),
        hasLength(1),
      );
      await DiagnosticLog.setEnabled(true);
      await DiagnosticLog.record(
        DiagnosticKind.pointer,
        DiagnosticStage.send,
        DiagnosticResult.ok,
      );
      await DiagnosticLog.clear();
      await tester.pump(const Duration(seconds: 1));
      expect(
        calls.where((call) => call.method == 'diagnosticEvent'),
        hasLength(1),
      );
    },
  );
  testWidgets('settings toggles session logging and displays and clears logs', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    final store = PadStore();
    await store.load();
    final prefs = await SharedPreferences.getInstance();
    final before = prefs.getKeys();
    await tester.pumpWidget(
      AgentPadApp(store: store, enableAutomaticUpdateChecks: false),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('设置'));
    await tester.pumpAndSettle();
    final toggle = find.byKey(const ValueKey('diagnostic-toggle'));
    await tester.ensureVisible(toggle);
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(toggle).value, false);
    await tester.tap(toggle);
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(toggle).value, true);
    expect(
      prefs
          .getKeys()
          .difference(before)
          .where((key) => key.contains('diagnostic')),
      isEmpty,
    );
    log = 'text send ok data=[redacted]';
    await tester.ensureVisible(find.byKey(const ValueKey('diagnostic-view')));
    await tester.tap(find.byKey(const ValueKey('diagnostic-view')));
    await tester.pumpAndSettle();
    expect(find.text(log), findsOneWidget);
    await tester.tap(find.text('关闭'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('diagnostic-clear')));
    await tester.pumpAndSettle();
    expect(log, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pumpAndSettle();
  });
}
