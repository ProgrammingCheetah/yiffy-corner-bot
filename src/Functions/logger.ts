import { v4 as uuid } from 'uuid';
function getID() {
    return uuid();
}

export default { getID }; 