/**
 * Regra ESLint autoral `max/single-line-start-tag`:
 * Exige que toda tag de abertura no `<template>` de um SFC Vue tenha seu nome,
 * atributos, props, diretivas, eventos e fechamento `>` ou `/>` na mesma linha.
 */
export default {
    meta: {
        type: 'layout',
        docs: {
            description: 'Exige que tags de abertura no <template> permaneçam em uma única linha.'
        },
        schema: [],
        messages: {
            multilineStartTag: 'A tag de abertura <{{name}}> deve permanecer inteiramente em uma única linha (AGENTS.md §3).'
        }
    },
    create(context) {
        const parserServices = context.sourceCode?.parserServices ?? context.parserServices;
        if (!parserServices || typeof parserServices.defineTemplateBodyVisitor !== 'function') {
            return {};
        }

        return parserServices.defineTemplateBodyVisitor({
            VElement(node) {
                const { startTag } = node;
                if (!startTag || !startTag.loc) {
                    return;
                }
                if (startTag.loc.start.line !== startTag.loc.end.line) {
                    context.report({
                        node: startTag,
                        loc: startTag.loc,
                        messageId: 'multilineStartTag',
                        data: {
                            name: node.rawName || node.name
                        }
                    });
                }
            }
        });
    }
};
