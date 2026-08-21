import 'dart:ui';

class TouchpadAction {
  const TouchpadAction({
    this.dx = 0,
    this.dy = 0,
    this.buttons = 0,
    this.wheel = 0,
    this.immediate = false,
  });

  final double dx;
  final double dy;
  final int buttons;
  final int wheel;
  final bool immediate;
}

class TouchpadGesture {
  static const movementThreshold = 6.0;
  final _points = <int, Offset>{};
  Offset? _start;
  Offset? _last;
  Offset? _multiCenter;
  var _travel = 0.0;
  var _wheel = 0.0;
  var _longPress = false;
  var _dragging = false;
  var _multi = false;
  var _didScroll = false;

  int get pointerCount => _points.length;

  List<TouchpadAction> down(int id, Offset position) {
    if (_points.isEmpty) {
      _reset();
      _start = position;
      _last = position;
    }
    _points[id] = position;
    if (_points.length < 2) return const [];

    _multi = true;
    _longPress = false;
    _multiCenter = _center;
    if (!_dragging) return const [];
    _dragging = false;
    return const [TouchpadAction(buttons: 0, immediate: true)];
  }

  bool armLongPress() {
    if (_multi || _points.length != 1 || _travel > movementThreshold) {
      return false;
    }
    _longPress = true;
    return true;
  }

  List<TouchpadAction> move(int id, Offset position) {
    if (!_points.containsKey(id)) return const [];
    if (_multi) return _moveMulti(id, position);

    final delta = position - _last!;
    _last = position;
    _points[id] = position;
    _travel = (position - _start!).distance > _travel
        ? (position - _start!).distance
        : _travel;
    if (delta == Offset.zero) return const [];

    final actions = <TouchpadAction>[];
    if (_longPress && _travel > movementThreshold && !_dragging) {
      _dragging = true;
      actions.add(const TouchpadAction(buttons: 1, immediate: true));
    }
    actions.add(
      TouchpadAction(dx: delta.dx, dy: delta.dy, buttons: _dragging ? 1 : 0),
    );
    return actions;
  }

  List<TouchpadAction> up(int id, Offset position) {
    if (!_points.containsKey(id)) return const [];
    if (_multi) {
      final actions = _points.length > 1
          ? _moveMulti(id, position)
          : <TouchpadAction>[];
      _points.remove(id);
      if (_points.isNotEmpty) return actions;
      if (!_didScroll && _travel <= movementThreshold) {
        actions.addAll(_click(2));
      }
      _reset();
      return actions;
    }

    _travel = (position - _start!).distance > _travel
        ? (position - _start!).distance
        : _travel;
    final actions = _dragging
        ? const [TouchpadAction(buttons: 0, immediate: true)]
        : _travel <= movementThreshold
        ? _click(_longPress ? 2 : 1)
        : const <TouchpadAction>[];
    _reset();
    return actions;
  }

  List<TouchpadAction> cancel() {
    final actions = _dragging
        ? const [TouchpadAction(buttons: 0, immediate: true)]
        : const <TouchpadAction>[];
    _reset();
    return actions;
  }

  List<TouchpadAction> _moveMulti(int id, Offset position) {
    final oldCenter = _multiCenter;
    _points[id] = position;
    if (_points.length < 2 || oldCenter == null) return const [];
    final center = _center;
    final delta = center - oldCenter;
    _multiCenter = center;
    _travel += delta.distance;
    _wheel += delta.dy;
    final actions = <TouchpadAction>[];
    final wheel = _wheel.truncate();
    if (wheel != 0) {
      _wheel -= wheel;
      _didScroll = true;
      actions.add(TouchpadAction(wheel: wheel));
    }
    return actions;
  }

  Offset get _center {
    var total = Offset.zero;
    for (final point in _points.values) {
      total += point;
    }
    return total / _points.length.toDouble();
  }

  List<TouchpadAction> _click(int button) => [
    TouchpadAction(buttons: button, immediate: true),
    const TouchpadAction(buttons: 0, immediate: true),
  ];

  void _reset() {
    _points.clear();
    _start = null;
    _last = null;
    _multiCenter = null;
    _travel = 0;
    _wheel = 0;
    _longPress = false;
    _dragging = false;
    _multi = false;
    _didScroll = false;
  }
}
