import 'package:flutter_test/flutter_test.dart';

import 'package:agentpad/touchpad.dart';

void main() {
  test('single tap clicks left', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    final actions = pad.up(1, const Offset(2, 1));
    expect(actions.map((a) => a.buttons), [1, 0]);
  });

  test('long press then move drags with left button', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    expect(pad.armLongPress(), isTrue);
    final move = pad.move(1, const Offset(12, 0));
    expect(move.first.buttons, 1);
    expect(move.first.immediate, isTrue);
    expect(move.last.dx, 12);
    expect(move.last.buttons, 1);
    expect(pad.up(1, const Offset(12, 0)).single.buttons, 0);
  });

  test('long press without meaningful movement clicks right', () {
    final pad = TouchpadGesture();
    pad.down(1, Offset.zero);
    pad.armLongPress();
    final actions = pad.up(1, const Offset(3, 2));
    expect(actions.map((a) => a.buttons), [2, 0]);
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
      [...first, ...second].fold(0, (sum, action) => sum + action.wheel),
      24,
    );
    expect(pad.up(1, const Offset(0, 24)), isEmpty);
    expect(pad.up(2, const Offset(20, 24)), isEmpty);
  });
}
