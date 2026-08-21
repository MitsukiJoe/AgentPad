import 'package:flutter/services.dart';

class VoiceInputDetector {
  bool _hasVoiceEvidence = false;

  bool update(TextEditingValue value, {bool voiceEvidence = false}) {
    final text = value.text;
    if (text.isEmpty ||
        !value.selection.isCollapsed ||
        value.selection.baseOffset != text.length) {
      reset();
      return false;
    }
    _hasVoiceEvidence |= voiceEvidence;
    final composing = value.composing.isValid && !value.composing.isCollapsed;
    return _hasVoiceEvidence && !composing;
  }

  void reset() => _hasVoiceEvidence = false;
}
