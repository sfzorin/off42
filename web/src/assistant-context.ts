type Position = { id: number; offset: number };
export type Selection = string | { start: Position; end: Position };
export type Command = Record<string, unknown> & { command: string };

export function contextCommand(selection: Selection, sheet?: number): Command {
  if (typeof selection === 'string') return { command: 'get_sheet_info', sheet, focus: selection };
  if (!selection.start?.id || !selection.end?.id) return { command: 'document_context' };
  return { command: 'document_context', start: selection.start, end: selection.end };
}

// Bound reads even when the model omits optional paging parameters. Native callers
// can still request an entire paragraph through edit42.
export function toolCommand(name: string, args: Record<string, unknown>, sheet?: number): Command {
  const command: Command = { command: name, ...args };
  if (sheet !== undefined) command.sheet = sheet;
  if (name === 'get_outline') command.limit ??= 16;
  if (name === 'read_range' || name === 'read_cells') command.length ??= 1600;
  return command;
}

export const readTools = new Set([
  'get_outline',
  'read_range',
  'find',
  'get_sheet_info',
  'read_cells',
  'find_cells',
  'aggregate_cells',
]);

// Reserve headroom in the encrypted continuation for subsequent turns. A large read
// returns an actionable, complete JSON object, so the model can retry a smaller window.
export function toolResult(content: string, calls: number, applied = false): string {
  const budget = Math.floor(16000 / calls);
  if (new TextEncoder().encode(content).length <= budget) return content;
  if (applied)
    return JSON.stringify({ applied: true, details_omitted: true, hint: 'Read the changed area to verify.' });
  return JSON.stringify({
    error:
      'Result too large. Request fewer paragraph IDs, cells or outline entries. For a single long paragraph or cell, reduce length and page using start.',
  });
}
