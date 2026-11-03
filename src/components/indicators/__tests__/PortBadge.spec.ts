import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import PortBadge from '@/components/indicators/PortBadge.vue';
import type { ListeningPort_Serialize } from '@/types/bindings';

function makeSamplePort(port: number): ListeningPort_Serialize {
    return {
        id: `pane-1:tcp:${port}`,
        tabId: 'tab-1',
        paneId: 'pane-1',
        ptyId: 'pty-1',
        port,
        protocol: 'tcp',
        addresses: [
            { family: 'ipv4', bindAddr: '127.0.0.1', isWildcard: false, isLoopback: true }
        ],
        browserUrl: `http://127.0.0.1:${port}/`,
        pid: 3000 + port,
        startTimeTicks: 1000,
        processName: 'vite',
        cmdlinePreview: 'vite',
        detectedAtMs: 1000
    };
}

describe('PortBadge.vue', () => {
    it('renderiza até 2 portas inline, +N overflow, emite open-browser e confirma kill-port', async () => {
        const p1 = makeSamplePort(5177);
        const p2 = makeSamplePort(8000);
        const p3 = makeSamplePort(9000);

        const wrapper = mount(PortBadge, {
            props: {
                ports: [p1, p2, p3]
            },
            global: { stubs: { MaxIcon: true } }
        });

        expect(wrapper.text()).toContain(':5177');
        expect(wrapper.text()).toContain(':8000');
        expect(wrapper.find('.port-badge__overflow').text()).toBe('+1');

        await wrapper.find('.port-badge__open-btn').trigger('click');
        expect(wrapper.emitted('open-browser')?.[0]).toEqual([p1]);

        await wrapper.find('.port-badge__kill-btn').trigger('click');
        expect(wrapper.find('.port-badge__confirm').exists()).toBe(true);

        await wrapper.find('.port-badge__confirm-yes').trigger('click');
        expect(wrapper.emitted('kill-port')?.[0]).toEqual([p1]);
    });
});
