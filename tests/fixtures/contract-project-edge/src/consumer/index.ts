import { apiHandler } from '../api/handler';
import { sharedUtil } from '../shared/types';

export function consumerMain(): string {
    const result = apiHandler();
    const data = sharedUtil();
    return `${result} - ${data.id}`;
}
