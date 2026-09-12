import 'package:flutter_test/flutter_test.dart';

import 'package:agentpad/touchpad.dart';

void main() {
  test('single tap clicks left', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    final actions = pad.up(1, const Offset(2, 1));
    expect(actions.map((a) => a.buttons), [1, 0]);
  });

  test('long press only presses left once movement crosses the threshold', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    expect(pad.armLongPress(), isTrue);
    expect(pad.move(1, const Offset(3, 2)), isEmpty);
    final move = pad.move(1, const Offset(12, 0));
    expect(move.map((a) => a.buttons), [1, 1]);
    expect(move.first.immediate, isTrue);
    expect(move.first.dx, 0);
    expect(move.last.dx, 12);
    expect(move.last.dy, 0);
    expect(pad.up(1, const Offset(12, 0)).single.buttons, 0);
  });

  test('long press without meaningful movement clicks right', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    expect(pad.armLongPress(), isTrue);
    expect(pad.move(1, const Offset(3, 2)), isEmpty);
    final actions = pad.up(1, const Offset(3, 2));
    expect(actions.map((a) => a.buttons), [2, 0]);
  });

  test('cancellation and a second finger release only an active drag', () {
    for (final dragged in [false, true]) {
      for (final secondFinger in [false, true]) {
        final pad = TouchpadGesture();
        pad.down(1, Offset.zero);
        pad.armLongPress();
        if (dragged) pad.move(1, const Offset(12, 0));
        final actions = secondFinger
            ? pad.down(2, const Offset(20, 0))
            : pad.cancel();
        expect(actions.map((a) => a.buttons), dragged ? [0] : <int>[]);
        expect(pad.cancel(), isEmpty);
      }
    }
  });

  test('moving past the threshold before long press keeps normal movement', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    expect(pad.move(1, const Offset(10, 0)).single.dx, 10);
    expect(pad.armLongPress(), isFalse);
    expect(pad.up(1, const Offset(10, 0)), isEmpty);
  });

  test('two finger tap clicks right without a trailing left click', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    pad.down(2, const Offset(20, 0));
    expect(pad.up(1, Offset.zero), isEmpty);
    final actions = pad.up(2, const Offset(20, 0));
    expect(actions.map((a) => a.buttons), [2, 0]);
  });

  test('two finger vertical motion scrolls without clicking', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    pad.down(2, const Offset(20, 0));
    final first = pad.move(1, const Offset(0, 24));
    final second = pad.move(2, const Offset(20, 24));
    expect(
      [...first, ...second].fold(0.0, (sum, action) => sum + action.wheel),
      24,
    );
    expect(pad.up(1, const Offset(0, 24)), isEmpty);
    expect(pad.up(2, const Offset(20, 24)), isEmpty);
  });

  test('two finger scrolling preserves subpixel motion', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    pad.down(2, const Offset(20, 0));
    expect(pad.move(1, const Offset(0, 0.5)).single.wheel, 0.25);
    expect(pad.move(2, const Offset(20, 0.5)).single.wheel, 0.25);
    expect(pad.up(1, const Offset(0, 0.5)), isEmpty);
    expect(pad.up(2, const Offset(20, 0.5)), isEmpty);
  });
}
