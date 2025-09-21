import { ApisauceInstance, create } from "apisauce";


function GetComponent(): ApisauceInstance {
    return create({
        baseURL: "https://e621.net/posts.json",
        headers: {
            Cookie: 'gw=seen',
            'User-Agent': 'PostSelector-ZielAnima/v0.3',
        },
    });
}

export default { GetComponent };
