import { formatDate, generateToken } from '../provider/index';

export const run = () => formatDate(new Date()) + generateToken();
