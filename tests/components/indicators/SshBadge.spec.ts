import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import SshBadge from '@/components/indicators/SshBadge.vue';
import type { TemplateConnection_Serialize } from '@/types/bindings';

describe('SshBadge.vue (R2 / R5)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });
    it('não renderiza elemento quando a conexão for local ou nula', () => {
        const wrapperLocal = mount(SshBadge, {
            props: {
                connection: { kind: 'local' }
            }
        });
        expect(wrapperLocal.find('.ssh-badge').exists()).toBe(false);

        const wrapperNull = mount(SshBadge, {
            props: {
                connection: null
            }
        });
        expect(wrapperNull.find('.ssh-badge').exists()).toBe(false);
    });

    it('renderiza o badge SSH com usuário e host corretos para conexão remota', () => {
        const connection: TemplateConnection_Serialize = {
            kind: 'ssh',
            host: '192.168.1.15',
            user: 'deploy',
            auth: {
                kind: 'password',
                credentialId: '01950000-0000-7000-8000-000000000001'
            }
        };

        const wrapper = mount(SshBadge, {
            props: {
                connection
            }
        });

        expect(wrapper.find('.ssh-badge').exists()).toBe(true);
        expect(wrapper.text()).toContain('SSH: deploy@192.168.1.15');
        expect(wrapper.attributes('title')).toBe(
            'Conexão SSH remota supervisionada: deploy@192.168.1.15'
        );
    });
});
