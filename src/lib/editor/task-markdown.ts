export type ParsedTaskTrigger = {
  checked: boolean;
};

const EXPLICIT_TASK_REGEX = /^- \[( |x|X)\] $/;
const EXPLICIT_TASK_LINE_REGEX = /^- \[( |x|X)\] (.+)$/;

export function parseExplicitTaskTrigger(text: string): ParsedTaskTrigger | null {
  const match = EXPLICIT_TASK_REGEX.exec(text);
  if (!match) return null;
  return { checked: (match[1] ?? '').toLowerCase() === 'x' };
}

export function parseExplicitTaskLine(text: string): { checked: boolean; text: string } | null {
  const match = EXPLICIT_TASK_LINE_REGEX.exec(text);
  if (!match) return null;
  return {
    checked: (match[1] ?? '').toLowerCase() === 'x',
    text: match[2] ?? '',
  };
}
