import fs from 'node:fs';
import path from 'node:path';

const rootDir = process.cwd();
const manifestPath = path.join(rootDir, 'traceability-manifest.json');

if (!fs.existsSync(manifestPath)) {
    console.error(`[check:traceability] Manifesto ausente: ${manifestPath}`);
    process.exit(1);
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const expectedPhases = Array.from({ length: 17 }, (_, idx) =>
    String(idx + 1).padStart(2, '0')
);
const requiredAgentsSections = ['1', '2', '3', '4.1', '4.2', '5.1', '5.2', '6', '7', '8.1', '8.2', '8.3'];

const errors = [];
const seenPhases = new Set();
const seenSections = new Set();

for (const entry of manifest.phases ?? []) {
    seenPhases.add(entry.phase);

    for (const sec of entry.agents_sections ?? []) {
        seenSections.add(sec);
    }

    if (! Array.isArray(entry.modules) || entry.modules.length === 0) {
        errors.push(`Fase ${entry.phase} sem módulos mapeados.`);
    }

    for (const modRel of entry.modules ?? []) {
        const modAbs = path.join(rootDir, modRel);
        if (!fs.existsSync(modAbs)) {
            errors.push(`Fase ${entry.phase}: módulo inexistente '${modRel}'.`);
        }
    }

    if (!Array.isArray(entry.tests) || entry.tests.length === 0) {
        errors.push(`Fase ${entry.phase} sem testes automatizados mapeados.`);
    }

    for (const testRef of entry.tests ?? []) {
        const testAbs = path.join(rootDir, testRef.file);
        if (!fs.existsSync(testAbs)) {
            errors.push(`Fase ${entry.phase}: arquivo de teste inexistente '${testRef.file}'.`);
            continue;
        }
        const content = fs.readFileSync(testAbs, 'utf8');
        if (!testRef.symbol || !content.includes(testRef.symbol)) {
            errors.push(
                `Fase ${entry.phase}: símbolo de teste '${testRef.symbol}' não encontrado em '${testRef.file}'.`
            );
        }
    }
}

for (const ph of expectedPhases) {
    if (!seenPhases.has(ph)) {
        errors.push(`Fase obrigatória ${ph} ausente em traceability-manifest.json.`);
    }
}

for (const sec of requiredAgentsSections) {
    if (!seenSections.has(sec)) {
        errors.push(`Seção obrigatória '${sec}' do AGENTS.md sem cobertura em traceability-manifest.json.`);
    }
}

if (errors.length > 0) {
    console.error('[check:traceability] Falhas de rastreabilidade detectadas:');
    for (const err of errors) {
        console.error(`  - ${err}`);
    }
    process.exit(1);
}

console.log(
    `[check:traceability] OK — ${seenPhases.size} fases e ${seenSections.size} seções do AGENTS.md verificadas.`
);
