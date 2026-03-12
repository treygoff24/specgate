import { formatDate } from '../provider/internal/helpers/format';
import { generateToken } from '../provider/internal/services/auth/token';

export const run = () => formatDate(new Date()) + generateToken();
