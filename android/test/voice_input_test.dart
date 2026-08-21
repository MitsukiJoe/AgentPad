import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:agentpad/voice_input.dart';

TextEditingValue edit(String text, {TextRange composing = TextRange.empty}) {
  return TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
    composing: composing,
  );
}

void main() {
  test('ordinary character typing is not a voice candidate', () {
    final detector = VoiceInputDetector();
    expect(detector.update(edit('你')), isFalse);
    expect(detector.update(edit('你好')), isFalse);
    expect(detector.update(edit('你好呀')), isFalse);
  });

  test('typed composition never becomes a voice candidate', () {
    final detector = VoiceInputDetector();
    expect(
      detector.update(edit('注', composing: const TextRange(start: 0, end: 1))),
      isFalse,
    );
    expect(
      detector.update(edit('注目', composing: const TextRange(start: 0, end: 2))),
      isFalse,
    );
    expect(detector.update(edit('注目')), isFalse);
  });

  test('long paste never becomes a voice candidate', () {
    final detector = VoiceInputDetector();
    expect(detector.update(edit('这是一段很长的粘贴文字')), isFalse);
  });

  test('voice evidence arms after composition finishes', () {
    final detector = VoiceInputDetector();
    expect(
      detector.update(
        edit('测', composing: const TextRange(start: 0, end: 1)),
        voiceEvidence: true,
      ),
      isFalse,
    );
    expect(
      detector.update(edit('测试', composing: const TextRange(start: 0, end: 2))),
      isFalse,
    );
    expect(detector.update(edit('测试了')), isTrue);
  });

  test('voice evidence arms a direct final commit', () {
    final detector = VoiceInputDetector();
    expect(detector.update(edit('好的'), voiceEvidence: true), isTrue);
  });

  test('selection movement cancels voice evidence', () {
    final detector = VoiceInputDetector();
    expect(
      detector.update(
        edit('语音输入', composing: const TextRange(start: 0, end: 4)),
        voiceEvidence: true,
      ),
      isFalse,
    );
    expect(
      detector.update(
        const TextEditingValue(
          text: '语音输入',
          selection: TextSelection.collapsed(offset: 2),
        ),
      ),
      isFalse,
    );
    expect(detector.update(edit('语音输入')), isFalse);
  });

  test('reset prevents a later commit from reusing voice evidence', () {
    final detector = VoiceInputDetector();
    expect(
      detector.update(
        edit('测', composing: const TextRange(start: 0, end: 1)),
        voiceEvidence: true,
      ),
      isFalse,
    );
    detector.reset();
    expect(detector.update(edit('测试')), isFalse);
  });
}
