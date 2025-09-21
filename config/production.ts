
const variants = (orig: string) => [orig, orig.toUpperCase(), orig.toLowerCase()];

const commands = {
    unauthenticated: {},
    authenticated: {
        list: variants('List'),
        count: variants('Count'),
        get: {
            posts: variants('getPosts'),
            byRank: variants('getByRank'),
            promising: variants('getPromising'),
            withTags: variants('getWithTags'),
        }
    },
    owner: {
        add: variants('Add'),
        remove: variants('Remove'),
        removeFromSaved: variants('RemoveFromSaved'),
    }
};


const bot = {
    token: './production/token.txt'
};

const owner = {
    id: 1402476143
};

const channel = {
    at: './production/at.txt'
};

const db = "./storage/db.sqlite";

const logs = {
    dir: '/home/yagdrassyl/bots/logs'
};

const name = './production/name.txt';

const cookies = {
    a: './production/cookie_a.txt',
    b: './production/cookie_b.txt',
};
export default {
    bot,
    db,
    owner,
    channel,
    commands,
    logs,
    name,
    cookies,
};
