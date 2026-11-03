import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import HighlightedText from '@/components/search/HighlightedText.vue';

describe('HighlightedText.vue (CA-14)', () => {
    it('renderiza payload XSS <img src=x onerror=alert(1)> como texto literal sem criar elementos HTML injetados', () => {
        const malicious = '<img src=x onerror=alert(1)><script>window.__pwned = true</script>';
        const wrapper = mount(HighlightedText, {
            props: {
                segments: [
                    { text: 'Comando: ', matched: false },
                    { text: malicious, matched: true },
                    { text: ` e sufixo ${malicious}`, matched: false }
                ]
            }
        });

        expect(wrapper.find('img').exists()).toBe(false);
        expect(wrapper.find('script').exists()).toBe(false);
        expect(wrapper.findAll('mark')).toHaveLength(1);
        expect(wrapper.find('mark').text()).toBe(malicious);
        expect(wrapper.text()).toContain(malicious);
    });

    it('mescla segmentos adjacentes com mesmo estado matched e preserva emojis, acentos e RTL', () => {
        const wrapper = mount(HighlightedText, {
            props: {
                segments: [
                    { text: 'Olá ', matched: false },
                    { text: '🚀 ', matched: false },
                    { text: 'مرحبا', matched: true },
                    { text: ' mundo', matched: true }
                ]
            }
        });

        const marks = wrapper.findAll('mark');
        expect(marks).toHaveLength(1);
        expect(marks[0]?.text()).toBe('مرحبا mundo');
        expect(wrapper.text()).toBe('Olá 🚀 مرحبا mundo');
    });
});
