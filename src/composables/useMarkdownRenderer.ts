/**
 * Renderizador e sanitizador seguro de Markdown para HTML (em conformidade com a CSP do MaxTerminal).
 *
 * Bloqueia integralmente `<script>`, `<iframe>`, `<object>`, `<embed>`, `<style>`, `<form>`,
 * atributos `on*`, URLs `javascript:`, `vbscript:` e `data:` (exceto imagens seguras se aplicável,
 * mas por padrão aceita apenas `https://`, `http://`, `mailto:` e `#` em links).
 */

function escapeHtml(raw: string): string {
    return raw
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}

function stripDangerousHtmlBlocks(input: string): string {
    // Remove blocos completos de tags perigosas caso estejam embutidas no markdown bruto.
    return input
        .replace(/<\s*(script|iframe|object|embed|style|form|svg|math|applet|meta|link|base)\b[^>]*>[\s\S]*?<\s*\/\s*\1\s*>/gi, '')
        .replace(/<\s*(script|iframe|object|embed|style|form|svg|math|applet|meta|link|base)\b[^>]*\/?\s*>/gi, '');
}

function sanitizeHref(rawUrl: string): string | null {
    const trimmed = rawUrl.trim().replace(/[\u0000-\u001F\u007F-\u009F\s]+/g, '');
    if (!trimmed) {
        return null;
    }
    const lower = trimmed.toLowerCase();
    if (
        lower.startsWith('javascript:') ||
        lower.startsWith('vbscript:') ||
        lower.startsWith('data:') ||
        lower.startsWith('file:')
    ) {
        return null;
    }
    if (
        lower.startsWith('https://') ||
        lower.startsWith('http://') ||
        lower.startsWith('mailto:') ||
        lower.startsWith('#')
    ) {
        return escapeHtml(trimmed);
    }
    return null;
}

function renderInlineMarkdown(escapedLine: string): string {
    // 1. Extrai código inline (`code`) em placeholders para não aplicar formatação dentro dele.
    const codeSlots: string[] = [];
    let working = escapedLine.replace(/`([^`]+)`/g, (_match, code: string) => {
        const idx = codeSlots.length;
        codeSlots.push(`<code class="md-inline-code">${code}</code>`);
        return `\u0000CODE_${idx}\u0000`;
    });

    // 2. Links [texto](url)
    working = working.replace(
        /\[([^\]]+)\]\(((?:[^()]+|\([^()]*\))+)\)/g,
        (_match, label: string, rawHref: string) => {
            // Reverte entidades básicas na URL antes de validar protocolo
            const unescapedHref = rawHref
                .replace(/&amp;/g, '&')
                .replace(/&quot;/g, '"')
                .replace(/&#39;/g, "'");
            const safeHref = sanitizeHref(unescapedHref);
            if (!safeHref) {
                return `<span class="md-unsafe-link">${label}</span>`;
            }
            return `<a href="${safeHref}" target="_blank" rel="noopener noreferrer" class="md-link">${label}</a>`;
        }
    );

    // 3. Negrito (**texto**), Itálico (*texto*) e Tachado (~~texto~~)
    working = working.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    working = working.replace(/\*([^*]+)\*/g, '<em>$1</em>');
    working = working.replace(/~~([^~]+)~~/g, '<del>$1</del>');

    // 4. Restaura slots de código inline
    working = working.replace(/\u0000CODE_(\d+)\u0000/g, (_m, idxStr: string) => {
        const idx = Number.parseInt(idxStr, 10);
        return codeSlots[idx] ?? '';
    });

    return working;
}

export function renderSafeMarkdown(markdown: string): string {
    if (!markdown || !markdown.trim()) {
        return '';
    }

    const cleaned = stripDangerousHtmlBlocks(markdown.replace(/\r\n/g, '\n'));
    const lines = cleaned.split('\n');
    const out: string[] = [];

    let inCodeBlock = false;
    let codeBlockLang = '';
    let codeLines: string[] = [];
    let inUl = false;
    let inOl = false;
    let inBlockquote = false;

    const closeLists = (): void => {
        if (inUl) {
            out.push('</ul>');
            inUl = false;
        }
        if (inOl) {
            out.push('</ol>');
            inOl = false;
        }
        if (inBlockquote) {
            out.push('</blockquote>');
            inBlockquote = false;
        }
    };

    for (const rawLine of lines) {
        const fenceMatch = /^\s*(`{3,}|~{3,})([\w-]*)\s*$/.exec(rawLine);
        if (fenceMatch) {
            closeLists();
            if (!inCodeBlock) {
                inCodeBlock = true;
                codeBlockLang = escapeHtml(fenceMatch[2] || 'text');
                codeLines = [];
            } else {
                out.push(
                    `<pre class="md-code-block" data-lang="${codeBlockLang}"><code>${codeLines.join('\n')}</code></pre>`
                );
                inCodeBlock = false;
                codeBlockLang = '';
                codeLines = [];
            }
            continue;
        }

        if (inCodeBlock) {
            codeLines.push(escapeHtml(rawLine));
            continue;
        }

        const trimmed = rawLine.trim();
        if (!trimmed) {
            closeLists();
            continue;
        }

        const headingMatch = /^(#{1,6})\s+(.+)$/.exec(trimmed);
        if (headingMatch) {
            closeLists();
            const level = headingMatch[1]?.length ?? 2;
            const text = renderInlineMarkdown(escapeHtml(headingMatch[2] ?? ''));
            out.push(`<h${level} class="md-h${level}">${text}</h${level}>`);
            continue;
        }

        const taskMatch = /^[-*+]\s+\[([ xX])\]\s+(.+)$/.exec(trimmed);
        if (taskMatch) {
            if (inOl || inBlockquote) {
                closeLists();
            }
            if (!inUl) {
                out.push('<ul class="md-list md-task-list">');
                inUl = true;
            }
            const checked = (taskMatch[1] ?? '').toLowerCase() === 'x';
            const content = renderInlineMarkdown(escapeHtml(taskMatch[2] ?? ''));
            out.push(
                `<li class="md-task-item"><input type="checkbox" disabled ${checked ? 'checked' : ''} /> <span>${content}</span></li>`
            );
            continue;
        }

        const ulMatch = /^[-*+]\s+(.+)$/.exec(trimmed);
        if (ulMatch) {
            if (inOl || inBlockquote) {
                closeLists();
            }
            if (!inUl) {
                out.push('<ul class="md-list">');
                inUl = true;
            }
            out.push(`<li>${renderInlineMarkdown(escapeHtml(ulMatch[1] ?? ''))}</li>`);
            continue;
        }

        const olMatch = /^\d+\.\s+(.+)$/.exec(trimmed);
        if (olMatch) {
            if (inUl || inBlockquote) {
                closeLists();
            }
            if (!inOl) {
                out.push('<ol class="md-list">');
                inOl = true;
            }
            out.push(`<li>${renderInlineMarkdown(escapeHtml(olMatch[1] ?? ''))}</li>`);
            continue;
        }

        const quoteMatch = /^>\s?(.*)$/.exec(trimmed);
        if (quoteMatch) {
            if (inUl || inOl) {
                closeLists();
            }
            if (!inBlockquote) {
                out.push('<blockquote class="md-quote">');
                inBlockquote = true;
            }
            out.push(`<p>${renderInlineMarkdown(escapeHtml(quoteMatch[1] ?? ''))}</p>`);
            continue;
        }

        closeLists();
        out.push(`<p class="md-p">${renderInlineMarkdown(escapeHtml(trimmed))}</p>`);
    }

    if (inCodeBlock) {
        out.push(
            `<pre class="md-code-block" data-lang="${codeBlockLang}"><code>${codeLines.join('\n')}</code></pre>`
        );
    }
    closeLists();

    return out.join('\n');
}

export function useMarkdownRenderer() {
    return {
        renderSafeMarkdown
    };
}
