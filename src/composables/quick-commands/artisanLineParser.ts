import type { ArtisanCommand, ArtisanOption, ArtisanRoot } from '@/types/bindings';

export interface LineToken {
    text: string;
    rawText: string;
    start: number;
    end: number;
    quoted: boolean;
}

export interface ArtisanLine {
    invoker: string;
    command: string | null;
    current: LineToken;
    usedOptions: Set<string>;
    positionalsBefore: number;
    mode: 'option' | 'argument-hint' | 'none';
}

interface ScanOutcome {
    tokensBeforeCursor: LineToken[];
    currentToken: LineToken;
    hasShellOperator: boolean;
    unclosedQuote: boolean;
}

/**
 * Faz o scan léxico da linha até `cursor` (estendendo o token atual até o próximo espaço fora de aspas),
 * detectando operadores de encadeamento/redirecionamento shell fora de aspas.
 */
function scanLine(input: string, cursor: number): ScanOutcome {
    const tokens: LineToken[] = [];
    let i = 0;
    const len = input.length;

    while (i < cursor && /\s/.test(input[i] ?? '')) {
        i += 1;
    }

    let currentStart = cursor;
    let currentEnd = cursor;
    let currentText = '';
    let currentRaw = '';
    let currentQuoted = false;
    let foundCurrent = false;

    while (i < len) {
        if (i >= cursor && !foundCurrent) {
            break;
        }

        // Verifica operadores shell fora de aspas antes do cursor
        if (i < cursor) {
            const ch = input[i];
            const next = input[i + 1];
            if (
                ch === '|' ||
                ch === ';' ||
                ch === '>' ||
                ch === '<' ||
                ch === '\n' ||
                ch === '\r' ||
                ch === '`' ||
                (ch === '&' && next === '&') ||
                (ch === '$' && next === '(')
            ) {
                return {
                    tokensBeforeCursor: [],
                    currentToken: { text: '', rawText: '', start: cursor, end: cursor, quoted: false },
                    hasShellOperator: true,
                    unclosedQuote: false
                };
            }
        }

        const tokStart = i;
        let text = '';
        let quoted = false;
        let quoteChar: "'" | '"' | null = null;

        while (i < len) {
            const ch = input[i]!;
            if (quoteChar === null) {
                if (/\s/.test(ch)) {
                    break;
                }
                if (
                    ch === '|' ||
                    ch === ';' ||
                    ch === '>' ||
                    ch === '<' ||
                    ch === '\n' ||
                    ch === '\r' ||
                    ch === '`' ||
                    (ch === '&' && input[i + 1] === '&') ||
                    (ch === '$' && input[i + 1] === '(')
                ) {
                    if (i < cursor) {
                        return {
                            tokensBeforeCursor: [],
                            currentToken: {
                                text: '',
                                rawText: '',
                                start: cursor,
                                end: cursor,
                                quoted: false
                            },
                            hasShellOperator: true,
                            unclosedQuote: false
                        };
                    }
                    break;
                }
                if (ch === "'" || ch === '"') {
                    quoted = true;
                    quoteChar = ch;
                    i += 1;
                    continue;
                }
                if (ch === '\\' && i + 1 < len) {
                    text += input[i + 1]!;
                    i += 2;
                    continue;
                }
                text += ch;
                i += 1;
            } else if (quoteChar === "'") {
                if (ch === "'") {
                    quoteChar = null;
                    i += 1;
                } else {
                    text += ch;
                    i += 1;
                }
            } else {
                if (ch === '"') {
                    quoteChar = null;
                    i += 1;
                } else if (ch === '\\' && i + 1 < len) {
                    text += input[i + 1]!;
                    i += 2;
                } else if (ch === '`' || (ch === '$' && input[i + 1] === '(')) {
                    if (i < cursor) {
                        return {
                            tokensBeforeCursor: [],
                            currentToken: {
                                text: '',
                                rawText: '',
                                start: cursor,
                                end: cursor,
                                quoted: false
                            },
                            hasShellOperator: true,
                            unclosedQuote: false
                        };
                    }
                    text += ch;
                    i += 1;
                } else {
                    text += ch;
                    i += 1;
                }
            }
        }

        const tokEnd = i;
        const rawText = input.slice(tokStart, tokEnd);

        if (quoteChar !== null && tokStart < cursor) {
            return {
                tokensBeforeCursor: [],
                currentToken: { text: '', rawText: '', start: cursor, end: cursor, quoted: true },
                hasShellOperator: false,
                unclosedQuote: true
            };
        }

        if (cursor >= tokStart && cursor <= tokEnd) {
            currentStart = tokStart;
            currentEnd = tokEnd;
            const sliceUpToCursor = input.slice(tokStart, cursor);
            currentText = unquotePartial(sliceUpToCursor);
            currentRaw = rawText;
            currentQuoted = quoted;
            foundCurrent = true;
            break;
        }

        if (tokEnd < cursor) {
            tokens.push({
                text,
                rawText,
                start: tokStart,
                end: tokEnd,
                quoted
            });
        }

        while (i < len && /\s/.test(input[i] ?? '')) {
            i += 1;
        }
    }

    const currentToken: LineToken = foundCurrent
        ? {
            text: currentText,
            rawText: currentRaw,
            start: currentStart,
            end: currentEnd,
            quoted: currentQuoted
        }
        : {
            text: '',
            rawText: '',
            start: cursor,
            end: cursor,
            quoted: false
        };

    return {
        tokensBeforeCursor: tokens,
        currentToken,
        hasShellOperator: false,
        unclosedQuote: false
    };
}

function unquotePartial(raw: string): string {
    let out = '';
    let i = 0;
    let quote: "'" | '"' | null = null;
    while (i < raw.length) {
        const ch = raw[i]!;
        if (quote === null) {
            if (ch === "'" || ch === '"') {
                quote = ch;
                i += 1;
                continue;
            }
            if (ch === '\\' && i + 1 < raw.length) {
                out += raw[i + 1]!;
                i += 2;
                continue;
            }
            out += ch;
            i += 1;
        } else if (ch === quote) {
            quote = null;
            i += 1;
        } else {
            out += ch;
            i += 1;
        }
    }
    return out;
}

function matchInvokerTokens(tokens: LineToken[], root: ArtisanRoot): number | null {
    const expected = root.invokerTokens;
    if (
        expected.length > 0 &&
        tokens.length >= expected.length &&
        expected.every((exp, idx) => tokens[idx]?.text === exp)
    ) {
        return expected.length;
    }

    // Formas canônicas equivalentes quando na própria raiz (ex.: `./artisan`, `php artisan`, `php ./artisan`, `sail artisan`, `./vendor/bin/sail artisan`, `vendor/bin/sail artisan`)
    const first = tokens[0]?.text;
    const second = tokens[1]?.text;
    const isRootLevel =
        expected.length === 2 &&
        (expected[1] === 'artisan' || expected[1] === './artisan');

    if (isRootLevel) {
        if (first === './artisan') {
            return 1;
        }
        if (
            (first === 'php' ||
                first === 'sail' ||
                first === './vendor/bin/sail' ||
                first === 'vendor/bin/sail') &&
            (second === 'artisan' || second === './artisan')
        ) {
            return 2;
        }
    }

    return null;
}

function findOptionByName(cmd: ArtisanCommand | undefined, flagName: string): ArtisanOption | undefined {
    if (!cmd) {
        return undefined;
    }
    const normalized = flagName.startsWith('--') ? flagName : `--${flagName}`;
    return cmd.options?.find((o) => o.name === normalized);
}

function findOptionByShortcut(
    cmd: ArtisanCommand | undefined,
    shortToken: string
): ArtisanOption | undefined {
    if (!cmd || !shortToken.startsWith('-') || shortToken.startsWith('--')) {
        return undefined;
    }
    const letter = shortToken.slice(1);
    if (letter.length !== 1) {
        return undefined;
    }
    return cmd.options?.find((o) => {
        if (!o.shortcut) {
            return false;
        }
        return o.shortcut
            .split('|')
            .map((s) => s.trim().replace(/^-+/, ''))
            .includes(letter);
    });
}

/**
 * Analisa a linha de comando Artisan até `cursor` respeitando `root.invokerTokens`,
 * identificando o subcomando ativo, as opções já utilizadas e o modo de autocompletar (`option`, `argument-hint` ou `none`).
 */
export function parseArtisanLine(
    input: string,
    cursor: number,
    root: ArtisanRoot,
    resolveCommand?: (name: string) => ArtisanCommand | undefined
): ArtisanLine | null {
    if (cursor < 0 || cursor > input.length) {
        return null;
    }

    const scan = scanLine(input, cursor);
    if (scan.hasShellOperator || scan.unclosedQuote) {
        return null;
    }

    const invokerLen = matchInvokerTokens(scan.tokensBeforeCursor, root);
    if (invokerLen === null) {
        return null;
    }

    const invokerText = scan.tokensBeforeCursor
        .slice(0, invokerLen)
        .map((t) => t.rawText)
        .join(' ');

    const afterInvoker = scan.tokensBeforeCursor.slice(invokerLen);

    // Se ainda não há um token de subcomando completo antes do cursor, o usuário ainda está digitando o nome do comando
    if (afterInvoker.length === 0) {
        return {
            invoker: invokerText,
            command: null,
            current: scan.currentToken,
            usedOptions: new Set(),
            positionalsBefore: 0,
            mode: 'none'
        };
    }

    const commandToken = afterInvoker[0]!;
    if (commandToken.text.startsWith('-')) {
        return {
            invoker: invokerText,
            command: null,
            current: scan.currentToken,
            usedOptions: new Set(),
            positionalsBefore: 0,
            mode: 'none'
        };
    }

    const commandName = commandToken.text;
    const cmdDef = resolveCommand ? resolveCommand(commandName) : undefined;
    const usedOptions = new Set<string>();
    let positionalsBefore = 0;
    let endOfOptions = false;

    const prevArgs = afterInvoker.slice(1);
    let idx = 0;
    while (idx < prevArgs.length) {
        const tok = prevArgs[idx]!;
        if (endOfOptions) {
            positionalsBefore += 1;
            idx += 1;
            continue;
        }

        if (tok.text === '--' && !tok.quoted) {
            endOfOptions = true;
            idx += 1;
            continue;
        }

        if (tok.text.startsWith('--')) {
            const eqIdx = tok.text.indexOf('=');
            if (eqIdx !== -1) {
                usedOptions.add(tok.text.slice(0, eqIdx));
                idx += 1;
            } else {
                usedOptions.add(tok.text);
                const optMeta = findOptionByName(cmdDef, tok.text);
                if (
                    optMeta?.acceptValue === true &&
                    optMeta?.isValueRequired === true &&
                    idx + 1 < prevArgs.length &&
                    !prevArgs[idx + 1]!.text.startsWith('-')
                ) {
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            continue;
        }

        if (tok.text.startsWith('-') && tok.text.length > 1) {
            const eqIdx = tok.text.indexOf('=');
            const shortPart = eqIdx !== -1 ? tok.text.slice(0, eqIdx) : tok.text;
            const optMeta = findOptionByShortcut(cmdDef, shortPart);
            if (optMeta) {
                usedOptions.add(optMeta.name);
                if (
                    eqIdx === -1 &&
                    optMeta.acceptValue === true &&
                    optMeta.isValueRequired === true &&
                    idx + 1 < prevArgs.length &&
                    !prevArgs[idx + 1]!.text.startsWith('-')
                ) {
                    idx += 2;
                    continue;
                }
            }
            idx += 1;
            continue;
        }

        positionalsBefore += 1;
        idx += 1;
    }

    let mode: 'option' | 'argument-hint' | 'none' = 'none';
    if (!endOfOptions && scan.currentToken.text.startsWith('-')) {
        // Se o token atual já possui `=` (ex.: `--database=mysql`), o usuário está digitando o valor da opção
        if (!scan.currentToken.text.includes('=')) {
            mode = 'option';
        }
    } else if (scan.currentToken.text === '' && cmdDef && (cmdDef.arguments?.length ?? 0) > 0) {
        const args = cmdDef.arguments!;
        const lastArg = args[args.length - 1];
        if (positionalsBefore < args.length || lastArg?.isArray === true) {
            mode = 'argument-hint';
        }
    }

    return {
        invoker: invokerText,
        command: commandName,
        current: scan.currentToken,
        usedOptions,
        positionalsBefore,
        mode
    };
}
